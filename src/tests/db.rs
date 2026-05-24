use std::sync::Mutex;

use crate::session::channel_model::{ChannelSession, ClaudeSession, ClaudeSessionState};

// Tests touch SQLite on a temp path, so serialize them.
static DB_TEST_LOCK: Mutex<()> = Mutex::new(());

fn setup_temp_db() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let db_file = temp.path().join("sessions.db");
    crate::db::set_test_db_path(db_file);
    crate::db::init_schema().unwrap();
    temp
}

fn insert_test_channel(id: &str) -> ChannelSession {
    let cs = ChannelSession {
        id: id.to_string(),
        title: "Test Channel".to_string(),
        source: crate::session::channel_model::SessionSource::WebUI,
        platform: "webui".to_string(),
        channel_id: id.to_string(),
        work_dir: "/tmp".to_string(),
        created_at: chrono::Utc::now(),
    };
    crate::db::insert_channel_session(&cs);
    cs
}

// ------------------------------------------------------------------
// ChannelSession CRUD
// ------------------------------------------------------------------

#[test]
fn test_insert_and_load_channel_session() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    let cs = ChannelSession::new_webui("Test", "/tmp");
    crate::db::insert_channel_session(&cs);

    let loaded = crate::db::load_channel_session_by_id(&cs.id);
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.id, cs.id);
    assert_eq!(loaded.title, cs.title);
    assert_eq!(loaded.work_dir, cs.work_dir);
    assert_eq!(loaded.platform, cs.platform);
}

#[test]
fn test_load_all_channel_sessions() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    let cs1 = ChannelSession::new_webui("A", "/a");
    let cs2 = ChannelSession::new_webui("B", "/b");
    crate::db::insert_channel_session(&cs1);
    crate::db::insert_channel_session(&cs2);

    let all = crate::db::load_all_channel_sessions();
    assert!(
        all.len() >= 2,
        "expected at least 2 channel sessions, got {}",
        all.len()
    );
}

#[test]
fn test_delete_channel_session() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    let cs = ChannelSession::new_webui("Del", "/tmp");
    crate::db::insert_channel_session(&cs);
    assert!(crate::db::load_channel_session_by_id(&cs.id).is_some());

    crate::db::delete_channel_session(&cs.id);
    assert!(crate::db::load_channel_session_by_id(&cs.id).is_none());
}

#[test]
fn test_update_channel_work_dir() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    let cs = ChannelSession::new_webui("Upd", "/old");
    crate::db::insert_channel_session(&cs);

    crate::db::update_channel_work_dir(&cs.id, "/new");
    let loaded = crate::db::load_channel_session_by_id(&cs.id).unwrap();
    assert_eq!(loaded.work_dir, "/new");
}

// ------------------------------------------------------------------
// ClaudeSession CRUD
// ------------------------------------------------------------------

#[test]
fn test_insert_and_load_claude_session() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    insert_test_channel("chan-1");
    let s = ClaudeSession::new("chan-1", "S1", "/tmp");
    crate::db::insert_claude_session(&s);

    let loaded = crate::db::load_all_claude_sessions();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id, s.id);
    assert_eq!(loaded[0].title, "S1");
    assert_eq!(loaded[0].state, ClaudeSessionState::Stopped);
    assert!(!loaded[0].active);
}

#[test]
fn test_load_claude_sessions_by_channel_id() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    insert_test_channel("chan-a");
    insert_test_channel("chan-b");
    let s1 = ClaudeSession::new("chan-a", "S1", "/tmp");
    let s2 = ClaudeSession::new("chan-a", "S2", "/tmp");
    let s3 = ClaudeSession::new("chan-b", "S3", "/tmp");
    crate::db::insert_claude_session(&s1);
    crate::db::insert_claude_session(&s2);
    crate::db::insert_claude_session(&s3);

    let a = crate::db::load_claude_sessions_by_channel_id("chan-a");
    assert_eq!(a.len(), 2);

    let b = crate::db::load_claude_sessions_by_channel_id("chan-b");
    assert_eq!(b.len(), 1);

    let c = crate::db::load_claude_sessions_by_channel_id("chan-none");
    assert!(c.is_empty());
}

#[test]
fn test_delete_claude_session() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    insert_test_channel("chan-1");
    let s = ClaudeSession::new("chan-1", "S1", "/tmp");
    crate::db::insert_claude_session(&s);
    assert_eq!(crate::db::load_all_claude_sessions().len(), 1);

    crate::db::delete_claude_session(&s.id);
    assert!(crate::db::load_all_claude_sessions().is_empty());
}

#[test]
fn test_update_claude_session_active() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    insert_test_channel("chan-1");
    let s = ClaudeSession::new("chan-1", "S1", "/tmp");
    crate::db::insert_claude_session(&s);

    crate::db::update_claude_session_active(&s.id, true);
    let loaded = crate::db::load_all_claude_sessions().pop().unwrap();
    assert!(loaded.active);
}

#[test]
fn test_update_claude_session_state() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    insert_test_channel("chan-1");
    let s = ClaudeSession::new("chan-1", "S1", "/tmp");
    crate::db::insert_claude_session(&s);

    crate::db::update_claude_session_state(&s.id, "dead");
    let loaded = crate::db::load_all_claude_sessions().pop().unwrap();
    assert_eq!(loaded.state, ClaudeSessionState::Dead);
}

#[test]
fn test_update_claude_session_stopped_at() {
    let _guard = DB_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _temp = setup_temp_db();

    insert_test_channel("chan-1");
    let s = ClaudeSession::new("chan-1", "S1", "/tmp");
    crate::db::insert_claude_session(&s);

    let now = chrono::Utc::now();
    crate::db::update_claude_session_stopped_at(&s.id, Some(now));
    let loaded = crate::db::load_all_claude_sessions().pop().unwrap();
    assert!(loaded.stopped_at.is_some());
}
