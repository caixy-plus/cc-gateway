use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::session::channel_model::{ChannelSession, ClaudeSession, ClaudeSessionState, SessionSource};
use crate::session::model::Session;

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

    // New tables
    conn.execute(
        "CREATE TABLE IF NOT EXISTS channel_sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            source TEXT NOT NULL,
            platform TEXT NOT NULL,
            channel_id TEXT,
            work_dir TEXT NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS claude_sessions (
            id TEXT PRIMARY KEY,
            channel_session_id TEXT NOT NULL,
            title TEXT NOT NULL,
            work_dir TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL DEFAULT 'stopped',
            claude_session_id TEXT,
            created_at TEXT NOT NULL,
            stopped_at TEXT,
            FOREIGN KEY (channel_session_id) REFERENCES channel_sessions(id)
        )",
        [],
    )?;

    // Legacy table for backward compatibility (create if not exists)
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

    // Migrate old sessions table data if it has data
    migrate_legacy_sessions(&conn)?;

    info!("SQLite session database initialized at {:?}", db_path());
    Ok(())
}

fn migrate_legacy_sessions(conn: &Connection) -> Result<()> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='sessions'",
        [],
        |row| row.get(0),
    )?;

    if count == 0 {
        return Ok(());
    }

    let legacy_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sessions",
        [],
        |row| row.get(0),
    )?;

    if legacy_count == 0 {
        return Ok(());
    }

    info!("Migrating {} legacy sessions to new schema", legacy_count);

    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id FROM sessions"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i32>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
        ))
    })?;

    for row in rows {
        let (id, title, source_str, platform, chat_id, work_dir, active, created_at, csid) = row?;

        // Create ChannelSession
        let channel_id = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO channel_sessions (id, title, source, platform, channel_id, work_dir, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![&channel_id, &title, &source_str, &platform, chat_id.as_deref().unwrap_or(&id), &work_dir, &created_at],
        )?;

        // Create ClaudeSession
        conn.execute(
            "INSERT OR IGNORE INTO claude_sessions (id, channel_session_id, title, work_dir, active, state, claude_session_id, created_at, stopped_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![&id, &channel_id, &title, &work_dir, active, if active != 0 { "active" } else { "stopped" }, csid.as_deref(), &created_at, if active != 0 { None } else { Some(created_at.clone()) }],
        )?;
    }

    // Rename legacy table to backup
    let _ = conn.execute("ALTER TABLE sessions RENAME TO sessions_legacy", []);
    info!("Legacy sessions table migrated and renamed to sessions_legacy");

    Ok(())
}

// ------------------------------------------------------------------
// ChannelSession CRUD
// ------------------------------------------------------------------

pub fn insert_channel_session(channel: &ChannelSession) {
    if let Err(e) = try_insert_channel_session(channel) {
        warn!("Failed to persist channel session {}: {}", channel.id, e);
    }
}

fn try_insert_channel_session(channel: &ChannelSession) -> Result<()> {
    let conn = open_conn()?;
    let source = source_to_str(&channel.source);
    conn.execute(
        "INSERT OR REPLACE INTO channel_sessions (id, title, source, platform, channel_id, work_dir, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            channel.id,
            channel.title,
            source,
            channel.platform,
            channel.channel_id,
            channel.work_dir,
            channel.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn delete_channel_session(id: &str) {
    if let Err(e) = try_delete_channel_session(id) {
        warn!("Failed to delete channel session {}: {}", id, e);
    }
}

fn try_delete_channel_session(id: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute("DELETE FROM channel_sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_channel_work_dir(id: &str, work_dir: &str) {
    if let Err(e) = try_update_channel_work_dir(id, work_dir) {
        warn!("Failed to update work_dir for channel session {}: {}", id, e);
    }
}

fn try_update_channel_work_dir(id: &str, work_dir: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE channel_sessions SET work_dir = ?1 WHERE id = ?2",
        params![work_dir, id],
    )?;
    Ok(())
}

pub fn load_all_channel_sessions() -> Vec<ChannelSession> {
    match try_load_all_channel_sessions() {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load channel sessions from DB: {}", e);
            Vec::new()
        }
    }
}

fn try_load_all_channel_sessions() -> Result<Vec<ChannelSession>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, channel_id, work_dir, created_at FROM channel_sessions"
    )?;
    let rows = stmt.query_map([], |row| {
        let source_str: String = row.get(2)?;
        let created_at_str: String = row.get(6)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        Ok(ChannelSession {
            id: row.get(0)?,
            title: row.get(1)?,
            source: str_to_source(&source_str),
            platform: row.get(3)?,
            channel_id: row.get(4)?,
            work_dir: row.get(5)?,
            created_at,
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse channel session row: {}", e),
        }
    }
    Ok(sessions)
}

pub fn load_channel_session_by_id(id: &str) -> Option<ChannelSession> {
    match try_load_channel_session_by_id(id) {
        Ok(session) => session,
        Err(e) => {
            error!("Failed to load channel session {} from DB: {}", id, e);
            None
        }
    }
}

fn try_load_channel_session_by_id(id: &str) -> Result<Option<ChannelSession>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, channel_id, work_dir, created_at FROM channel_sessions WHERE id = ?1"
    )?;
    let mut rows = stmt.query_map(params![id], |row| {
        let source_str: String = row.get(2)?;
        let created_at_str: String = row.get(6)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        Ok(ChannelSession {
            id: row.get(0)?,
            title: row.get(1)?,
            source: str_to_source(&source_str),
            platform: row.get(3)?,
            channel_id: row.get(4)?,
            work_dir: row.get(5)?,
            created_at,
        })
    })?;

    if let Some(row) = rows.next() {
        Ok(Some(row?))
    } else {
        Ok(None)
    }
}

// ------------------------------------------------------------------
// ClaudeSession CRUD
// ------------------------------------------------------------------

pub fn insert_claude_session(session: &ClaudeSession) {
    if let Err(e) = try_insert_claude_session(session) {
        warn!("Failed to persist Claude session {}: {}", session.id, e);
    }
}

fn try_insert_claude_session(session: &ClaudeSession) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO claude_sessions (id, channel_session_id, title, work_dir, active, state, claude_session_id, created_at, stopped_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            session.id,
            session.channel_session_id,
            session.title,
            session.work_dir,
            if session.active { 1 } else { 0 },
            session.state.to_string(),
            session.claude_session_id.as_deref(),
            session.created_at.to_rfc3339(),
            session.stopped_at.map(|t| t.to_rfc3339()),
        ],
    )?;
    Ok(())
}

pub fn delete_claude_session(id: &str) {
    if let Err(e) = try_delete_claude_session(id) {
        warn!("Failed to delete Claude session {}: {}", id, e);
    }
}

fn try_delete_claude_session(id: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute("DELETE FROM claude_sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_claude_session_active(id: &str, active: bool) {
    if let Err(e) = try_update_claude_session_active(id, active) {
        warn!("Failed to update active for Claude session {}: {}", id, e);
    }
}

fn try_update_claude_session_active(id: &str, active: bool) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE claude_sessions SET active = ?1 WHERE id = ?2",
        params![if active { 1 } else { 0 }, id],
    )?;
    Ok(())
}

pub fn update_claude_session_state(id: &str, state: &str) {
    if let Err(e) = try_update_claude_session_state(id, state) {
        warn!("Failed to update state for Claude session {}: {}", id, e);
    }
}

fn try_update_claude_session_state(id: &str, state: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE claude_sessions SET state = ?1 WHERE id = ?2",
        params![state, id],
    )?;
    Ok(())
}

pub fn update_claude_session_claude_id(id: &str, claude_session_id: Option<&str>) {
    if let Err(e) = try_update_claude_session_claude_id(id, claude_session_id) {
        warn!("Failed to update claude_session_id for Claude session {}: {}", id, e);
    }
}

fn try_update_claude_session_claude_id(id: &str, claude_session_id: Option<&str>) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE claude_sessions SET claude_session_id = ?1 WHERE id = ?2",
        params![claude_session_id, id],
    )?;
    Ok(())
}

pub fn update_claude_session_stopped_at(id: &str, stopped_at: Option<chrono::DateTime<chrono::Utc>>) {
    if let Err(e) = try_update_claude_session_stopped_at(id, stopped_at) {
        warn!("Failed to update stopped_at for Claude session {}: {}", id, e);
    }
}

pub fn update_claude_session_updated_at(id: &str, updated_at: Option<chrono::DateTime<chrono::Utc>>) {
    if let Err(e) = try_update_claude_session_updated_at(id, updated_at) {
        warn!("Failed to update updated_at for Claude session {}: {}", id, e);
    }
}

fn try_update_claude_session_stopped_at(id: &str, stopped_at: Option<chrono::DateTime<chrono::Utc>>) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE claude_sessions SET stopped_at = ?1 WHERE id = ?2",
        params![stopped_at.map(|t| t.to_rfc3339()), id],
    )?;
    Ok(())
}

fn try_update_claude_session_updated_at(id: &str, updated_at: Option<chrono::DateTime<chrono::Utc>>) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE claude_sessions SET updated_at = ?1 WHERE id = ?2",
        params![updated_at.map(|t| t.to_rfc3339()), id],
    )?;
    Ok(())
}

pub fn load_all_claude_sessions() -> Vec<ClaudeSession> {
    match try_load_all_claude_sessions() {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load Claude sessions from DB: {}", e);
            Vec::new()
        }
    }
}

fn try_load_all_claude_sessions() -> Result<Vec<ClaudeSession>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, channel_session_id, title, work_dir, active, state, claude_session_id, created_at, stopped_at FROM claude_sessions"
    )?;
    let rows = stmt.query_map([], |row| {
        let state_str: String = row.get(5)?;
        let created_at_str: String = row.get(7)?;
        let stopped_at_str: Option<String> = row.get(8)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let stopped_at = stopped_at_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok()
        });
        Ok(ClaudeSession {
            id: row.get(0)?,
            channel_session_id: row.get(1)?,
            title: row.get(2)?,
            work_dir: row.get(3)?,
            active: row.get::<_, i32>(4)? != 0,
            state: state_str.parse().unwrap_or(ClaudeSessionState::Stopped),
            claude_session_id: row.get(6)?,
            created_at,
            stopped_at,
            updated_at: None,
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse Claude session row: {}", e),
        }
    }
    Ok(sessions)
}

pub fn load_claude_sessions_by_channel_id(channel_id: &str) -> Vec<ClaudeSession> {
    match try_load_claude_sessions_by_channel_id(channel_id) {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load Claude sessions for channel {} from DB: {}", channel_id, e);
            Vec::new()
        }
    }
}

fn try_load_claude_sessions_by_channel_id(channel_id: &str) -> Result<Vec<ClaudeSession>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, channel_session_id, title, work_dir, active, state, claude_session_id, created_at, stopped_at FROM claude_sessions WHERE channel_session_id = ?1"
    )?;
    let rows = stmt.query_map(params![channel_id], |row| {
        let state_str: String = row.get(5)?;
        let created_at_str: String = row.get(7)?;
        let stopped_at_str: Option<String> = row.get(8)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let stopped_at = stopped_at_str.and_then(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok()
        });
        Ok(ClaudeSession {
            id: row.get(0)?,
            channel_session_id: row.get(1)?,
            title: row.get(2)?,
            work_dir: row.get(3)?,
            active: row.get::<_, i32>(4)? != 0,
            state: state_str.parse().unwrap_or(ClaudeSessionState::Stopped),
            claude_session_id: row.get(6)?,
            created_at,
            stopped_at,
        })
    })?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse Claude session row: {}", e),
        }
    }
    Ok(sessions)
}

// ------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------

fn source_to_str(source: &SessionSource) -> &'static str {
    match source {
        SessionSource::WebUI => "webui",
        SessionSource::Feishu => "feishu",
        SessionSource::Telegram => "telegram",
        SessionSource::TUI => "tui",
    }
}

fn str_to_source(s: &str) -> SessionSource {
    match s {
        "feishu" => SessionSource::Feishu,
        "telegram" => SessionSource::Telegram,
        "tui" => SessionSource::TUI,
        _ => SessionSource::WebUI,
    }
}

// ------------------------------------------------------------------
// Legacy Session CRUD (backward compatibility)
// ------------------------------------------------------------------

pub fn insert_session(session: &Session) {
    if let Err(e) = try_insert_session(session) {
        warn!("Failed to persist legacy session {}: {}", session.id, e);
    }
}

fn try_insert_session(session: &Session) -> Result<()> {
    let conn = open_conn()?;
    let source = source_to_str(&session.source);
    conn.execute(
        "INSERT OR REPLACE INTO sessions_legacy (id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id)
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
        warn!("Failed to delete legacy session {}: {}", id, e);
    }
}

fn try_delete_session(id: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute("DELETE FROM sessions_legacy WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_active(id: &str, active: bool) {
    if let Err(e) = try_update_active(id, active) {
        warn!("Failed to update active for legacy session {}: {}", id, e);
    }
}

fn try_update_active(id: &str, active: bool) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE sessions_legacy SET active = ?1 WHERE id = ?2",
        params![if active { 1 } else { 0 }, id],
    )?;
    Ok(())
}

pub fn update_work_dir(id: &str, work_dir: &str) {
    if let Err(e) = try_update_work_dir(id, work_dir) {
        warn!("Failed to update work_dir for legacy session {}: {}", id, e);
    }
}

fn try_update_work_dir(id: &str, work_dir: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE sessions_legacy SET work_dir = ?1 WHERE id = ?2",
        params![work_dir, id],
    )?;
    Ok(())
}

pub fn update_claude_session_id(id: &str, claude_session_id: Option<&str>) {
    if let Err(e) = try_update_claude_session_id(id, claude_session_id) {
        warn!("Failed to update claude_session_id for legacy session {}: {}", id, e);
    }
}

fn try_update_claude_session_id(id: &str, claude_session_id: Option<&str>) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE sessions_legacy SET claude_session_id = ?1 WHERE id = ?2",
        params![claude_session_id, id],
    )?;
    Ok(())
}

pub fn load_all_sessions() -> Vec<Session> {
    match try_load_all_sessions() {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load legacy sessions from DB: {}", e);
            Vec::new()
        }
    }
}

pub fn load_sessions_by_chat_id(chat_id: &str) -> Vec<Session> {
    match try_load_sessions_by_chat_id(chat_id) {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load legacy sessions for chat_id {} from DB: {}", chat_id, e);
            Vec::new()
        }
    }
}

fn try_load_all_sessions() -> Result<Vec<Session>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id FROM sessions_legacy"
    )?;
    load_sessions_from_stmt(&mut stmt, [])
}

fn try_load_sessions_by_chat_id(chat_id: &str) -> Result<Vec<Session>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, source, platform, chat_id, work_dir, active, created_at, claude_session_id FROM sessions_legacy WHERE chat_id = ?1"
    )?;
    load_sessions_from_stmt(&mut stmt, params![chat_id])
}

fn load_sessions_from_stmt<'a>(
    stmt: &mut rusqlite::Statement<'a>,
    params: impl rusqlite::Params,
) -> Result<Vec<Session>> {
    let rows = stmt.query_map(params, |row| {
        let source_str: String = row.get(2)?;
        let source = str_to_source(&source_str);
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
            Err(e) => warn!("Failed to parse legacy session row: {}", e),
        }
    }
    Ok(sessions)
}
