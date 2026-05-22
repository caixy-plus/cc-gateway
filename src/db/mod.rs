use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use tracing::{error, info, warn};

use crate::session::model::{Session, SessionSource};

fn db_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cc-gateway")
        .join("sessions.db")
}

fn open_conn() -> Result<Connection> {
    let path = db_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(&path)?;
    Ok(conn)
}

pub fn init_schema() -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            source TEXT NOT NULL,
            platform TEXT NOT NULL,
            chat_id TEXT,
            work_dir TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL,
            claude_session_id TEXT
        )",
        [],
    )?;
    // Migrate: add claude_session_id if table exists from older version
    let _ = conn.execute(
        "ALTER TABLE sessions ADD COLUMN claude_session_id TEXT",
        [],
    );
    info!("SQLite session database initialized at {:?}", db_path());
    Ok(())
}

pub fn insert_session(session: &Session) {
    if let Err(e) = try_insert_session(session) {
        warn!("Failed to persist session {}: {}", session.id, e);
    }
}

fn try_insert_session(session: &Session) -> Result<()> {
    let conn = open_conn()?;
    let source = match session.source {
        SessionSource::WebUI => "webui",
        SessionSource::Feishu => "feishu",
        SessionSource::Telegram => "telegram",
    };
    conn.execute(
        "INSERT OR REPLACE INTO sessions (id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            session.id,
            session.title,
            source,
            session.platform,
            session.chat_id,
            session.work_dir,
            if session.active { 1 } else { 0 },
            session.created_at.to_rfc3339(),
            session.claude_session_id.as_deref(),
        ],
    )?;
    Ok(())
}

pub fn delete_session(id: &str) {
    if let Err(e) = try_delete_session(id) {
        warn!("Failed to delete session {}: {}", id, e);
    }
}

fn try_delete_session(id: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_active(id: &str, active: bool) {
    if let Err(e) = try_update_active(id, active) {
        warn!("Failed to update active for session {}: {}", id, e);
    }
}

fn try_update_active(id: &str, active: bool) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE sessions SET active = ?1 WHERE id = ?2",
        params![if active { 1 } else { 0 }, id],
    )?;
    Ok(())
}

pub fn update_work_dir(id: &str, work_dir: &str) {
    if let Err(e) = try_update_work_dir(id, work_dir) {
        warn!("Failed to update work_dir for session {}: {}", id, e);
    }
}

fn try_update_work_dir(id: &str, work_dir: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE sessions SET work_dir = ?1 WHERE id = ?2",
        params![work_dir, id],
    )?;
    Ok(())
}

pub fn update_claude_session_id(id: &str, claude_session_id: Option<&str>) {
    if let Err(e) = try_update_claude_session_id(id, claude_session_id) {
        warn!("Failed to update claude_session_id for session {}: {}", id, e);
    }
}

fn try_update_claude_session_id(id: &str, claude_session_id: Option<&str>) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE sessions SET claude_session_id = ?1 WHERE id = ?2",
        params![claude_session_id, id],
    )?;
    Ok(())
}

pub fn load_all_sessions() -> Vec<Session> {
    match try_load_all_sessions() {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load sessions from DB: {}", e);
            Vec::new()
        }
    }
}

pub fn load_sessions_by_chat_id(chat_id: &str) -> Vec<Session> {
    match try_load_sessions_by_chat_id(chat_id) {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load sessions for chat_id {} from DB: {}", chat_id, e);
            Vec::new()
        }
    }
}

fn try_load_all_sessions() -> Result<Vec<Session>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id FROM sessions"
    )?;
    let rows = stmt.query_map([], |row| {
        let source_str: String = row.get(2)?;
        let source = match source_str.as_str() {
            "feishu" => SessionSource::Feishu,
            "telegram" => SessionSource::Telegram,
            _ => SessionSource::WebUI,
        };
        let created_at_str: String = row.get(7)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let claude_session_id: Option<String> = row.get(8)?;
        Ok(Session {
            id: row.get(0)?,
            title: row.get(1)?,
            source,
            platform: row.get(3)?,
            chat_id: row.get(4)?,
            work_dir: row.get(5)?,
            active: row.get::<_, i32>(6)? != 0,
            created_at,
            claude_session_id,
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse session row: {}", e),
        }
    }
    Ok(sessions)
}

fn try_load_sessions_by_chat_id(chat_id: &str) -> Result<Vec<Session>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id FROM sessions WHERE chat_id = ?1"
    )?;
    let rows = stmt.query_map(params![chat_id], |row| {
        let source_str: String = row.get(2)?;
        let source = match source_str.as_str() {
            "feishu" => SessionSource::Feishu,
            "telegram" => SessionSource::Telegram,
            _ => SessionSource::WebUI,
        };
        let created_at_str: String = row.get(7)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let claude_session_id: Option<String> = row.get(8)?;
        Ok(Session {
            id: row.get(0)?,
            title: row.get(1)?,
            source,
            platform: row.get(3)?,
            chat_id: row.get(4)?,
            work_dir: row.get(5)?,
            active: row.get::<_, i32>(6)? != 0,
            created_at,
            claude_session_id,
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse session row: {}", e),
        }
    }
    Ok(sessions)
}
