use anyhow::Result;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use tracing::{error, info, warn};

use crate::session::channel_model::{
    AgentSession, AgentSessionState, ChannelSession, SessionSource,
};

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

    // Legacy table: older versions used `claude_sessions`.
    // Never DROP it automatically: users may rely on the historical data.
    let legacy_exists: bool = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type='table' AND name='claude_sessions' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if legacy_exists {
        warn!(
            "Legacy table 'claude_sessions' exists; keeping it to avoid data loss. \
             It is not used by current versions."
        );
    }

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS channel_sessions (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            source TEXT NOT NULL,
            platform TEXT NOT NULL,
            channel_id TEXT,
            work_dir TEXT NOT NULL,
            default_provider TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS agent_sessions (
            id TEXT PRIMARY KEY,
            channel_session_id TEXT NOT NULL,
            provider TEXT NOT NULL DEFAULT 'claude',
            title TEXT NOT NULL,
            work_dir TEXT NOT NULL,
            active INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL DEFAULT 'stopped',
            provider_session_id TEXT,
            created_at TEXT NOT NULL,
            stopped_at TEXT,
            updated_at TEXT,
            FOREIGN KEY (channel_session_id) REFERENCES channel_sessions(id)
        );
        CREATE TABLE IF NOT EXISTS pending_pairings (
            pairing_code TEXT PRIMARY KEY,
            platform TEXT NOT NULL,
            chat_id TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS approved_chats (
            platform TEXT NOT NULL,
            chat_id TEXT NOT NULL,
            approved_at TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (platform, chat_id)
        );
        PRAGMA journal_mode=WAL;",
    )?;

    let _ = conn.execute(
        "ALTER TABLE channel_sessions ADD COLUMN default_provider TEXT",
        [],
    );

    // Backfill `enabled` for DBs created before the column existed.
    let _ = conn.execute(
        "ALTER TABLE approved_chats ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
        [],
    );

    info!("SQLite session database initialized at {:?}", db_path());
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
        "INSERT OR REPLACE INTO channel_sessions (id, title, source, platform, channel_id, work_dir, default_provider, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            channel.id,
            channel.title,
            source,
            channel.platform,
            channel.channel_id,
            channel.work_dir,
            channel.default_provider.as_deref(),
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
        warn!(
            "Failed to update work_dir for channel session {}: {}",
            id, e
        );
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

pub fn update_channel_default_provider(id: &str, provider: &str) {
    if let Err(e) = try_update_channel_default_provider(id, provider) {
        warn!(
            "Failed to update default_provider for channel session {}: {}",
            id, e
        );
    }
}

fn try_update_channel_default_provider(id: &str, provider: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE channel_sessions SET default_provider = ?1 WHERE id = ?2",
        params![provider, id],
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
        "SELECT id, title, source, platform, channel_id, work_dir, default_provider, created_at FROM channel_sessions",
    )?;
    let rows = stmt.query_map([], |row| {
        let source_str: String = row.get(2)?;
        let created_at_str: String = row.get(7)?;
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
            default_provider: row.get(6)?,
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

// ------------------------------------------------------------------
// AgentSession CRUD
// ------------------------------------------------------------------

pub fn insert_agent_session(session: &AgentSession) {
    if let Err(e) = try_insert_agent_session(session) {
        warn!("Failed to persist agent session {}: {}", session.id, e);
    }
}

fn try_insert_agent_session(session: &AgentSession) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO agent_sessions (id, channel_session_id, provider, title, work_dir, active, state, provider_session_id, created_at, stopped_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            session.id,
            session.channel_session_id,
            session.provider,
            session.title,
            session.work_dir,
            if session.active { 1 } else { 0 },
            session.state.to_string(),
            session.provider_session_id.as_deref(),
            session.created_at.to_rfc3339(),
            session.stopped_at.map(|t| t.to_rfc3339()),
            session.updated_at.map(|t| t.to_rfc3339()),
        ],
    )?;
    Ok(())
}

pub fn delete_agent_session(id: &str) {
    if let Err(e) = try_delete_agent_session(id) {
        warn!("Failed to delete agent session {}: {}", id, e);
    }
}

fn try_delete_agent_session(id: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute("DELETE FROM agent_sessions WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn load_all_agent_sessions() -> Vec<AgentSession> {
    match try_load_all_agent_sessions() {
        Ok(sessions) => sessions,
        Err(e) => {
            error!("Failed to load agent sessions from DB: {}", e);
            Vec::new()
        }
    }
}

fn parse_agent_session_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentSession> {
    let state_str: String = row.get(6)?;
    let created_at_str: String = row.get(8)?;
    let stopped_at_str: Option<String> = row.get(9)?;
    let updated_at_str: Option<String> = row.get(10)?;
    let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now());
    let stopped_at = stopped_at_str.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok()
    });
    let updated_at = updated_at_str.and_then(|s| {
        chrono::DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .ok()
    });
    Ok(AgentSession {
        id: row.get(0)?,
        channel_session_id: row.get(1)?,
        provider: row.get(2)?,
        title: row.get(3)?,
        work_dir: row.get(4)?,
        active: row.get::<_, i32>(5)? != 0,
        state: state_str.parse().unwrap_or(AgentSessionState::Stopped),
        provider_session_id: row.get(7)?,
        created_at,
        stopped_at,
        updated_at,
    })
}

fn try_load_all_agent_sessions() -> Result<Vec<AgentSession>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, channel_session_id, provider, title, work_dir, active, state, provider_session_id, created_at, stopped_at, updated_at FROM agent_sessions",
    )?;
    let rows = stmt.query_map([], parse_agent_session_row)?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse agent session row: {}", e),
        }
    }
    Ok(sessions)
}

pub fn load_agent_sessions_by_channel_id(channel_id: &str) -> Vec<AgentSession> {
    match try_load_agent_sessions_by_channel_id(channel_id) {
        Ok(sessions) => sessions,
        Err(e) => {
            error!(
                "Failed to load agent sessions for channel {} from DB: {}",
                channel_id, e
            );
            Vec::new()
        }
    }
}

fn try_load_agent_sessions_by_channel_id(channel_id: &str) -> Result<Vec<AgentSession>> {
    let conn = open_conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, channel_session_id, provider, title, work_dir, active, state, provider_session_id, created_at, stopped_at, updated_at FROM agent_sessions WHERE channel_session_id = ?1",
    )?;
    let rows = stmt.query_map(params![channel_id], parse_agent_session_row)?;

    let mut sessions = Vec::new();
    for row in rows {
        match row {
            Ok(session) => sessions.push(session),
            Err(e) => warn!("Failed to parse agent session row: {}", e),
        }
    }
    Ok(sessions)
}

pub fn reassign_agent_sessions_channel(from_channel_id: &str, to_channel_id: &str) -> usize {
    match try_reassign_agent_sessions_channel(from_channel_id, to_channel_id) {
        Ok(n) => n,
        Err(e) => {
            warn!(
                "Failed to reassign agent sessions from channel {} to {}: {}",
                from_channel_id, to_channel_id, e
            );
            0
        }
    }
}

fn try_reassign_agent_sessions_channel(
    from_channel_id: &str,
    to_channel_id: &str,
) -> Result<usize> {
    let conn = open_conn()?;
    let updated = conn.execute(
        "UPDATE agent_sessions SET channel_session_id = ?1 WHERE channel_session_id = ?2",
        params![to_channel_id, from_channel_id],
    )?;
    Ok(updated)
}

fn source_to_str(source: &SessionSource) -> &'static str {
    match source {
        SessionSource::WebUI => "webui",
        SessionSource::Feishu => "feishu",
        SessionSource::Telegram => "telegram",
        SessionSource::Qq => "qq",
    }
}

fn str_to_source(s: &str) -> SessionSource {
    match s {
        "webui" => SessionSource::WebUI,
        "feishu" => SessionSource::Feishu,
        "telegram" => SessionSource::Telegram,
        "qq" => SessionSource::Qq,
        other => {
            warn!(
                "Unknown SessionSource '{}' in DB, defaulting to WebUI",
                other
            );
            SessionSource::WebUI
        }
    }
}

// ------------------------------------------------------------------
// PendingPairing CRUD (works with raw fields; the pairing module owns the struct)
// ------------------------------------------------------------------

pub fn insert_pending_pairing(pairing_code: &str, platform: &str, chat_id: &str, created_at: &str) {
    if let Err(e) = try_insert_pending_pairing(pairing_code, platform, chat_id, created_at) {
        warn!("Failed to persist pending pairing {}: {}", pairing_code, e);
    }
}

fn try_insert_pending_pairing(
    pairing_code: &str,
    platform: &str,
    chat_id: &str,
    created_at: &str,
) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO pending_pairings (pairing_code, platform, chat_id, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![pairing_code, platform, chat_id, created_at],
    )?;
    Ok(())
}

pub fn delete_pending_pairing(pairing_code: &str) {
    if let Err(e) = try_delete_pending_pairing(pairing_code) {
        warn!("Failed to delete pending pairing {}: {}", pairing_code, e);
    }
}

fn try_delete_pending_pairing(pairing_code: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "DELETE FROM pending_pairings WHERE pairing_code = ?1",
        params![pairing_code],
    )?;
    Ok(())
}

pub fn load_all_pending_pairings() -> Vec<(String, String, String, String)> {
    match try_load_all_pending_pairings() {
        Ok(list) => list,
        Err(e) => {
            error!("Failed to load pending pairings from DB: {}", e);
            Vec::new()
        }
    }
}

fn try_load_all_pending_pairings() -> Result<Vec<(String, String, String, String)>> {
    let conn = open_conn()?;
    let mut stmt =
        conn.prepare("SELECT pairing_code, platform, chat_id, created_at FROM pending_pairings")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;

    let mut list = Vec::new();
    for row in rows {
        match row {
            Ok(p) => list.push(p),
            Err(e) => warn!("Failed to parse pending pairing row: {}", e),
        }
    }
    Ok(list)
}

// ------------------------------------------------------------------
// Approved chats CRUD (explicit allow-list for paired bot chats)
// ------------------------------------------------------------------

pub fn insert_approved_chat(platform: &str, chat_id: &str, approved_at: &str, enabled: bool) {
    if let Err(e) = try_insert_approved_chat(platform, chat_id, approved_at, enabled) {
        warn!(
            "Failed to persist approved chat {}:{}: {}",
            platform, chat_id, e
        );
    }
}

fn try_insert_approved_chat(
    platform: &str,
    chat_id: &str,
    approved_at: &str,
    enabled: bool,
) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO approved_chats (platform, chat_id, approved_at, enabled)
         VALUES (?1, ?2, ?3, ?4)",
        params![platform, chat_id, approved_at, enabled as i64],
    )?;
    Ok(())
}

pub fn set_approved_chat_enabled(platform: &str, chat_id: &str, enabled: bool) {
    if let Err(e) = try_set_approved_chat_enabled(platform, chat_id, enabled) {
        warn!(
            "Failed to update approved chat {}:{}: {}",
            platform, chat_id, e
        );
    }
}

fn try_set_approved_chat_enabled(platform: &str, chat_id: &str, enabled: bool) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "UPDATE approved_chats SET enabled = ?3 WHERE platform = ?1 AND chat_id = ?2",
        params![platform, chat_id, enabled as i64],
    )?;
    Ok(())
}

pub fn delete_approved_chat(platform: &str, chat_id: &str) {
    if let Err(e) = try_delete_approved_chat(platform, chat_id) {
        warn!(
            "Failed to delete approved chat {}:{}: {}",
            platform, chat_id, e
        );
    }
}

fn try_delete_approved_chat(platform: &str, chat_id: &str) -> Result<()> {
    let conn = open_conn()?;
    conn.execute(
        "DELETE FROM approved_chats WHERE platform = ?1 AND chat_id = ?2",
        params![platform, chat_id],
    )?;
    Ok(())
}

/// Returns (platform, chat_id, approved_at, enabled).
pub fn load_all_approved_chats() -> Vec<(String, String, String, bool)> {
    match try_load_all_approved_chats() {
        Ok(list) => list,
        Err(e) => {
            error!("Failed to load approved chats from DB: {}", e);
            Vec::new()
        }
    }
}

fn try_load_all_approved_chats() -> Result<Vec<(String, String, String, bool)>> {
    let conn = open_conn()?;
    let mut stmt =
        conn.prepare("SELECT platform, chat_id, approved_at, enabled FROM approved_chats")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)? != 0,
        ))
    })?;

    let mut list = Vec::new();
    for row in rows {
        match row {
            Ok(p) => list.push(p),
            Err(e) => warn!("Failed to parse approved chat row: {}", e),
        }
    }
    Ok(list)
}
