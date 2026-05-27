use std::io::Write;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use chrono::Utc;

use crate::daemon::cleaner::{
    clean_excess_sessions, clean_old_media_files, consolidate_stale_tui_channels_when,
    run_cleanup_cycle, select_sessions_to_keep, trim_log_file,
};
use crate::session::channel_model::{
    ChannelSession, ClaudeSession, ClaudeSessionState, SessionSource,
};
use crate::tests::helpers::TestEnv;

static CLEANER_TEST_LOCK: Mutex<()> = Mutex::new(());

fn cleaner_test_guard() -> std::sync::MutexGuard<'static, ()> {
    CLEANER_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ---------------------------------------------------------------------------
// trim_log_file
// ---------------------------------------------------------------------------

#[test]
fn trim_log_file_noop_when_missing() {
    let result = trim_log_file("/nonexistent/path/to/log.txt", 10, 100);
    assert!(result.is_ok());
    assert!(!result.unwrap());
}

#[test]
fn trim_log_file_noop_when_under_limit() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    writeln!(temp, "line1").unwrap();
    writeln!(temp, "line2").unwrap();
    let path = temp.path().to_str().unwrap();

    let result = trim_log_file(path, 10, 100).unwrap();
    assert!(!result);
}

#[test]
fn trim_log_file_trims_excess_lines() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    for i in 0..20 {
        writeln!(temp, "line{}", i).unwrap();
    }
    let path = temp.path().to_str().unwrap();

    let result = trim_log_file(path, 5, 100).unwrap();
    assert!(result);

    let content = std::fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "line15");
    assert_eq!(lines[4], "line19");
}

// ---------------------------------------------------------------------------
// media cleanup
// ---------------------------------------------------------------------------

#[test]
fn clean_old_media_files_removes_only_expired_files() {
    let _guard = cleaner_test_guard();
    let env = TestEnv::new();
    let media_dir = env.home().join(".cc-gateway").join("media");
    std::fs::create_dir_all(&media_dir).unwrap();

    let old_path = media_dir.join("old.bin");
    std::fs::write(&old_path, b"old").unwrap();
    let old_mtime = SystemTime::now() - Duration::from_secs(40 * 86400);
    std::fs::File::open(&old_path)
        .unwrap()
        .set_modified(old_mtime)
        .unwrap();

    let fresh_path = media_dir.join("fresh.bin");
    std::fs::write(&fresh_path, b"new").unwrap();

    let removed = clean_old_media_files(30).unwrap();
    assert_eq!(removed, 1);
    assert!(!old_path.exists());
    assert!(fresh_path.exists());
}

// ---------------------------------------------------------------------------
// session cleanup (isolated temp HOME + DB)
// ---------------------------------------------------------------------------

fn insert_telegram_channel(id: &str) {
    let channel = ChannelSession {
        id: id.to_string(),
        title: "Telegram".to_string(),
        source: SessionSource::Telegram,
        platform: "telegram".to_string(),
        channel_id: "chat-1".to_string(),
        work_dir: "/tmp".to_string(),
        created_at: Utc::now(),
    };
    crate::db::insert_channel_session(&channel);
}

fn insert_claude_session(
    channel_id: &str,
    id: &str,
    work_dir: &str,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
) {
    let session = ClaudeSession {
        id: id.to_string(),
        channel_session_id: channel_id.to_string(),
        provider: "claude".to_string(),
        title: format!("Session {}", id),
        work_dir: work_dir.to_string(),
        active: false,
        state: ClaudeSessionState::Stopped,
        provider_session_id: Some(format!("claude-{}", id)),
        claude_session_id: Some(format!("claude-{}", id)),
        created_at,
        stopped_at: Some(updated_at),
        updated_at: Some(updated_at),
    };
    crate::db::insert_claude_session(&session);
}

#[test]
fn select_sessions_to_keep_prefers_updated_at_over_created_at() {
    let _guard = cleaner_test_guard();
    let base = Utc::now();
    let sessions = vec![
        ClaudeSession {
            id: "stale-update".to_string(),
            channel_session_id: "ch".to_string(),
            provider: "claude".to_string(),
            title: "t".to_string(),
            work_dir: "/project/a".to_string(),
            active: false,
            state: ClaudeSessionState::Stopped,
            provider_session_id: None,
            claude_session_id: None,
            created_at: base,
            stopped_at: None,
            updated_at: Some(base - chrono::Duration::days(10)),
        },
        ClaudeSession {
            id: "fresh-update".to_string(),
            channel_session_id: "ch".to_string(),
            provider: "claude".to_string(),
            title: "t".to_string(),
            work_dir: "/project/a".to_string(),
            active: false,
            state: ClaudeSessionState::Stopped,
            provider_session_id: None,
            claude_session_id: None,
            created_at: base - chrono::Duration::days(10),
            stopped_at: None,
            updated_at: Some(base),
        },
    ];

    let keep = select_sessions_to_keep(&sessions, 1);
    assert_eq!(keep.len(), 1);
    assert!(keep.contains("fresh-update"));
}

#[test]
fn select_sessions_to_keep_pins_newest_per_work_dir_then_fills_by_time() {
    let _guard = cleaner_test_guard();
    let base = Utc::now();
    let sessions: Vec<ClaudeSession> = (0..5)
        .map(|i| ClaudeSession {
            id: format!("dir-a-{i}"),
            channel_session_id: "ch".to_string(),
            provider: "claude".to_string(),
            title: "t".to_string(),
            work_dir: "/project/a".to_string(),
            active: false,
            state: ClaudeSessionState::Stopped,
            provider_session_id: None,
            claude_session_id: None,
            created_at: base - chrono::Duration::seconds(i),
            stopped_at: None,
            updated_at: Some(base - chrono::Duration::seconds(i)),
        })
        .chain((0..5).map(|i| ClaudeSession {
            id: format!("dir-b-{i}"),
            channel_session_id: "ch".to_string(),
            provider: "claude".to_string(),
            title: "t".to_string(),
            work_dir: "/project/b".to_string(),
            active: false,
            state: ClaudeSessionState::Stopped,
            provider_session_id: None,
            claude_session_id: None,
            created_at: base - chrono::Duration::seconds(10 + i),
            stopped_at: None,
            updated_at: Some(base - chrono::Duration::seconds(10 + i)),
        }))
        .collect();

    let keep = select_sessions_to_keep(&sessions, 6);
    assert_eq!(keep.len(), 6);
    assert!(keep.contains("dir-a-0"));
    assert!(keep.contains("dir-b-0"));
    // Remaining slots are filled by global recency; dir-a is newer so dir-b extras drop first.
    assert!(!keep.contains("dir-b-4"));
}

#[test]
fn clean_excess_sessions_prunes_by_updated_at_not_created_at() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("tg-updated");

    let base = Utc::now();
    insert_claude_session(
        "tg-updated",
        "keep-me",
        "/tmp",
        base,
        base,
    );
    insert_claude_session(
        "tg-updated",
        "drop-me",
        "/tmp",
        base - chrono::Duration::hours(1),
        base - chrono::Duration::days(30),
    );
    for i in 0..30 {
        let created = base - chrono::Duration::seconds(i + 2);
        insert_claude_session(
            "tg-updated",
            &format!("filler-{i:02}"),
            &format!("/other/{i:02}"),
            created,
            created,
        );
    }

    clean_excess_sessions(30);

    let remaining = crate::db::load_claude_sessions_by_channel_id("tg-updated");
    assert_eq!(remaining.len(), 30);
    assert!(remaining.iter().any(|s| s.id == "keep-me"));
    assert!(!remaining.iter().any(|s| s.id == "drop-me"));
}

#[test]
fn clean_excess_sessions_respects_configured_cap_clamped_to_hundred() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("tg-cap-max");

    let base = Utc::now();
    for i in 0..110 {
        let at = base - chrono::Duration::seconds(i);
        insert_claude_session("tg-cap-max", &format!("cap-{i:03}"), "/tmp", at, at);
    }

    clean_excess_sessions(200);

    let remaining = crate::db::load_claude_sessions_by_channel_id("tg-cap-max");
    assert_eq!(remaining.len(), 100);
}

#[test]
fn clean_excess_sessions_respects_configured_cap_clamped_to_ten() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("tg-cap");

    let base = Utc::now();
    for i in 0..15 {
        let at = base - chrono::Duration::seconds(i);
        insert_claude_session("tg-cap", &format!("cap-{i:02}"), "/tmp", at, at);
    }

    clean_excess_sessions(5);

    let remaining = crate::db::load_claude_sessions_by_channel_id("tg-cap");
    assert_eq!(remaining.len(), 10);
}

#[test]
fn clean_excess_sessions_keeps_latest_thirty_per_channel() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("tg-channel");

    let base = Utc::now();
    for i in 0..35 {
        let created = base - chrono::Duration::seconds(i);
        insert_claude_session(
            "tg-channel",
            &format!("session-{i:02}"),
            "/tmp",
            created,
            created,
        );
    }

    let removed = clean_excess_sessions(30);
    assert_eq!(removed, 5);

    let remaining = crate::db::load_claude_sessions_by_channel_id("tg-channel");
    assert_eq!(remaining.len(), 30);
    let ids: Vec<String> = remaining.into_iter().map(|s| s.id).collect();
    for i in 0..30 {
        assert!(ids.contains(&format!("session-{i:02}")));
    }
    for i in 30..35 {
        assert!(!ids.contains(&format!("session-{i:02}")));
    }
}

#[test]
fn clean_excess_sessions_keeps_newest_per_work_dir_when_dirs_exceed_cap() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("tg-dirs");

    let base = Utc::now();
    for i in 0..35 {
        let at = base - chrono::Duration::seconds(i);
        insert_claude_session("tg-dirs", &format!("s-{i:02}"), &format!("/project/{i:02}"), at, at);
    }

    clean_excess_sessions(30);

    let remaining = crate::db::load_claude_sessions_by_channel_id("tg-dirs");
    assert_eq!(remaining.len(), 30);
    let work_dirs: std::collections::HashSet<_> =
        remaining.iter().map(|s| s.work_dir.as_str()).collect();
    assert_eq!(work_dirs.len(), 30);
    assert!(!remaining.iter().any(|s| s.id == "s-34"));
}

#[test]
fn clean_excess_sessions_keeps_at_least_one_session_per_work_dir() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("tg-multi");

    let base = Utc::now();
    for i in 0..20 {
        let at = base - chrono::Duration::seconds(i);
        insert_claude_session("tg-multi", &format!("a-{i:02}"), "/project/a", at, at);
    }
    for i in 0..20 {
        let at = base - chrono::Duration::seconds(100 + i);
        insert_claude_session("tg-multi", &format!("b-{i:02}"), "/project/b", at, at);
    }

    clean_excess_sessions(30);

    let remaining = crate::db::load_claude_sessions_by_channel_id("tg-multi");
    assert_eq!(remaining.len(), 30);
    assert!(remaining.iter().any(|s| s.id == "a-00"));
    assert!(remaining.iter().any(|s| s.id == "b-00"));
}

#[test]
fn consolidate_stale_tui_channels_preserves_claude_sessions() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();

    let canonical = ChannelSession {
        id: "tui-canonical".to_string(),
        title: "TUI".to_string(),
        source: SessionSource::TUI,
        platform: "tui".to_string(),
        channel_id: "tui".to_string(),
        work_dir: "/tmp".to_string(),
        created_at: Utc::now() - chrono::Duration::hours(1),
    };
    let stale = ChannelSession {
        id: "tui-stale".to_string(),
        title: "TUI old".to_string(),
        source: SessionSource::TUI,
        platform: "tui".to_string(),
        channel_id: "tui-history".to_string(),
        work_dir: "/tmp".to_string(),
        created_at: Utc::now(),
    };
    crate::db::insert_channel_session(&canonical);
    crate::db::insert_channel_session(&stale);
    let now = Utc::now();
    insert_claude_session("tui-stale", "shared-session", "/tmp", now, now);

    let removed = consolidate_stale_tui_channels_when(false);
    assert_eq!(removed, 1);
    assert!(
        !crate::db::load_all_channel_sessions()
            .iter()
            .any(|c| c.id == "tui-stale")
    );

    let sessions = crate::db::load_claude_sessions_by_channel_id("tui-canonical");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "shared-session");
}

// ---------------------------------------------------------------------------
// scheduled cleanup cycle (one tick)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_cleanup_cycle_trims_log_and_prunes_sessions() {
    let _guard = cleaner_test_guard();
    let _env = TestEnv::new();
    crate::db::init_schema().unwrap();
    insert_telegram_channel("cycle-channel");

    let base = Utc::now();
    for i in 0..32 {
        let created = base - chrono::Duration::seconds(i);
        insert_claude_session("cycle-channel", &format!("cycle-{i:02}"), "/tmp", created, created);
    }

    let mut log = tempfile::NamedTempFile::new().unwrap();
    for i in 0..12 {
        writeln!(log, "log-line-{}", i).unwrap();
    }
    let log_path = log.path().to_str().unwrap().to_string();

    let result = tokio::task::spawn_blocking(move || run_cleanup_cycle(&log_path, 5, 100, 0, 30))
        .await
        .unwrap()
        .unwrap();

    assert!(result.log_trimmed);
    assert_eq!(result.media_removed, 0);
    assert_eq!(result.excess_sessions_removed, 2);
    assert_eq!(
        crate::db::load_claude_sessions_by_channel_id("cycle-channel").len(),
        30
    );

    let content = std::fs::read_to_string(log.path()).unwrap();
    assert_eq!(content.lines().count(), 5);
}
