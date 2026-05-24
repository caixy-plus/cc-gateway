use crate::daemon::cleaner::{
    clean_excess_sessions, clean_old_media_files, clean_tui_sessions, trim_log_file,
};
use std::io::Write;

// ---------------------------------------------------------------------------
// trim_log_file tests
// ---------------------------------------------------------------------------

#[test]
fn test_trim_log_file_noop_when_missing() {
    let result = trim_log_file("/nonexistent/path/to/log.txt", 10, 100);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), false);
}

#[test]
fn test_trim_log_file_noop_when_under_limit() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    writeln!(temp, "line1").unwrap();
    writeln!(temp, "line2").unwrap();
    let path = temp.path().to_str().unwrap();

    let result = trim_log_file(path, 10, 100).unwrap();
    assert_eq!(result, false);
}

#[test]
fn test_trim_log_file_trims_excess_lines() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    for i in 0..20 {
        writeln!(temp, "line{}", i).unwrap();
    }
    let path = temp.path().to_str().unwrap();

    let result = trim_log_file(path, 5, 100).unwrap();
    assert_eq!(result, true);

    let content = std::fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "line15");
    assert_eq!(lines[4], "line19");
}

#[test]
fn test_trim_log_file_zero_max_lines() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    writeln!(temp, "line1").unwrap();
    let path = temp.path().to_str().unwrap();

    let result = trim_log_file(path, 0, 100).unwrap();
    assert_eq!(result, false);
}

#[test]
fn test_trim_log_file_triggers_on_size() {
    let mut temp = tempfile::NamedTempFile::new().unwrap();
    let chunk = "x".repeat(1024);
    for _ in 0..2200 {
        writeln!(temp, "{}", chunk).unwrap();
    }
    let path = temp.path().to_str().unwrap();

    let result = trim_log_file(path, usize::MAX, 1).unwrap();
    assert_eq!(result, true);
}

// ---------------------------------------------------------------------------
// Real DB cleanup tests — operate on ~/.cc-gateway/sessions.db
// ---------------------------------------------------------------------------

/// Print current DB state for inspection.
#[test]
fn test_show_cleanup_state() {
    let channels = crate::db::load_all_channel_sessions();
    println!("=== Channels ({}) ===", channels.len());
    for ch in &channels {
        let sessions = crate::db::load_claude_sessions_by_channel_id(&ch.id);
        println!(
            "  [{}] platform={} sessions={} work_dir={}",
            &ch.id[..ch.id.len().min(8)],
            ch.platform,
            sessions.len(),
            ch.work_dir,
        );
    }
}

/// Run clean_excess_sessions on real DB and report what was deleted.
#[test]
fn test_clean_excess_sessions_real() {
    let before_channels = crate::db::load_all_channel_sessions();
    let mut before_total = 0usize;
    for ch in &before_channels {
        if ch.platform != "tui" {
            before_total += crate::db::load_claude_sessions_by_channel_id(&ch.id).len();
        }
    }
    println!("Before: {} non-TUI sessions across {} channels", before_total, before_channels.len());

    let removed = clean_excess_sessions();
    println!("Removed: {} excess sessions", removed);

    let after_channels = crate::db::load_all_channel_sessions();
    let mut after_total = 0usize;
    for ch in &after_channels {
        if ch.platform != "tui" {
            let n = crate::db::load_claude_sessions_by_channel_id(&ch.id).len();
            println!("  [{}] {} sessions remaining", ch.platform, n);
            after_total += n;
        }
    }
    println!("After: {} non-TUI sessions", after_total);
    assert!(after_total <= before_total);
}

/// Run clean_tui_sessions on real DB.
#[test]
fn test_clean_tui_sessions_real() {
    let tui_running = crate::daemon::cleaner::is_tui_running();
    println!("TUI running: {}", tui_running);

    let removed = clean_tui_sessions();
    println!("Removed: {} TUI sessions (channels + claude_sessions)", removed);
}

/// Run media cleanup on real media dir.
#[test]
fn test_clean_old_media_real() {
    let result = clean_old_media_files(30);
    match result {
        Ok(n) => println!("Removed {} old media files", n),
        Err(e) => println!("Media cleanup failed: {}", e),
    }
}
