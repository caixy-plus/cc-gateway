use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use anyhow::Result;
use tracing::{error, info, warn};

/// Trim log file to retain at most `max_lines` lines.
/// Returns `true` if trimming was performed.
pub fn trim_log_file(path: &str, max_lines: usize, max_size_mb: usize) -> Result<bool> {
    if max_lines == 0 {
        return Ok(false);
    }

    let expanded = shellexpand::tilde(path).to_string();
    let path = PathBuf::from(expanded);

    if !path.exists() {
        return Ok(false);
    }

    let metadata = std::fs::metadata(&path)?;
    let size_mb = metadata.len() / (1024 * 1024);

    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut lines = VecDeque::new();
    let mut total_lines = 0usize;

    for line_result in reader.lines() {
        let line = line_result?;
        total_lines += 1;
        if lines.len() == max_lines {
            lines.pop_front();
        }
        lines.push_back(line);
    }

    if total_lines <= max_lines && size_mb <= max_size_mb as u64 {
        return Ok(false);
    }

    let output = lines.into_iter().collect::<Vec<_>>().join("\n");
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, &output)?;
    std::fs::rename(&tmp_path, &path)?;

    info!(
        "Trimmed log file from {} lines to {} lines (size was {}MB, limit {}MB)",
        total_lines,
        total_lines.min(max_lines),
        size_mb,
        max_size_mb
    );

    Ok(true)
}

/// Media directory under the cc-gateway config dir.
pub fn media_dir() -> PathBuf {
    dirs::home_dir()
        .map(|p| p.join(".cc-gateway").join("media"))
        .unwrap_or_else(|| PathBuf::from("/tmp/cc-gateway/media"))
}

/// Remove media files older than `retention_days`.
/// Returns the number of files removed.
pub fn clean_old_media_files(retention_days: u64) -> Result<usize> {
    let dir = media_dir();
    if !dir.exists() {
        return Ok(0);
    }

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(retention_days * 86400))
        .unwrap_or(SystemTime::UNIX_EPOCH);

    let mut removed = 0usize;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to read media dir {:?}: {}", dir, e);
            return Ok(0);
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        match entry.metadata() {
            Ok(meta) => {
                if let Ok(modified) = meta.modified() {
                    if modified < cutoff {
                        match fs::remove_file(&path) {
                            Ok(()) => {
                                info!("Removed old media file: {:?}", path);
                                removed += 1;
                            }
                            Err(e) => warn!("Failed to remove media file {:?}: {}", path, e),
                        }
                    }
                }
            }
            Err(e) => warn!("Failed to read metadata for {:?}: {}", path, e),
        }
    }

    if removed > 0 {
        info!("Cleaned {} media files older than {} days", removed, retention_days);
    }

    Ok(removed)
}

/// Known cc-gateway subcommands that are NOT the TUI interactive mode.
const MANAGEMENT_SUBCMDS: &[&str] = &[
    "_daemon", "start", "stop", "restart", "status", "log", "enable", "disable",
];

/// Parse `ps -eo args` output and classify each cc-gateway process.
/// Returns (tui_count, daemon_count, mgmt_count).
fn classify_cc_gateway_processes() -> (usize, usize, usize) {
    let output = match std::process::Command::new("ps")
        .args(["-eo", "args"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return (0, 0, 0),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tui = 0usize;
    let mut daemon = 0usize;
    let mut mgmt = 0usize;

    for line in stdout.lines() {
        // Find cc-gateway processes: last path component is "cc-gateway"
        let binary_name = line
            .split_whitespace()
            .next()
            .and_then(|p| p.rsplit('/').next());
        if binary_name != Some("cc-gateway") {
            continue;
        }

        // Skip test binaries (contain "/deps/" in path)
        if line.contains("/deps/") {
            continue;
        }

        let args: Vec<&str> = line.split_whitespace().collect();
        if args.len() == 1 {
            // Just the binary — TUI interactive mode
            tui += 1;
        } else if args[1] == "_daemon" {
            daemon += 1;
        } else if MANAGEMENT_SUBCMDS.contains(&args[1]) {
            mgmt += 1;
        }
        // else: unknown subcommand, ignore
    }

    (tui, daemon, mgmt)
}

/// Check whether a `cc-gateway` TUI process is currently running.
/// The TUI process is `cc-gateway` launched with no arguments (interactive mode).
pub fn is_tui_running() -> bool {
    let (tui, _, _) = classify_cc_gateway_processes();
    tui > 0
}

/// Check whether the `cc-gateway` daemon process is currently running.
#[allow(dead_code)]
pub fn is_daemon_running() -> bool {
    let (_, daemon, _) = classify_cc_gateway_processes();
    daemon > 0
}

/// Clean up TUI channel_sessions that are no longer in use.
/// When the TUI process is gone, remove the channel_session and its bound claude_sessions.
pub fn clean_tui_sessions() -> usize {
    let channels = crate::db::load_all_channel_sessions();
    let tui_active = is_tui_running();
    let mut removed = 0usize;

    for channel in &channels {
        if channel.platform != "tui" {
            continue;
        }

        if tui_active {
            continue;
        }

        // TUI is not running — clean up this channel's sessions
        let claude_sessions =
            crate::db::load_claude_sessions_by_channel_id(&channel.id);

        // Delete history files and claude_sessions first (FK constraint)
        for cs in &claude_sessions {
            let file_id = cs.claude_session_id.as_ref().unwrap_or(&cs.id);
            if let Some(home) = dirs::home_dir() {
                let history_file = home
                    .join(".cc-gateway")
                    .join("history")
                    .join(format!("{}.jsonl", file_id));
                let _ = std::fs::remove_file(&history_file);
            }
            crate::db::delete_claude_session(&cs.id);
            removed += 1;
        }

        // Now safe to delete the channel_session
        crate::db::delete_channel_session(&channel.id);
    }

    removed
}

/// Keep only the most recent 30 claude_sessions per non-TUI channel.
/// Returns the number of sessions deleted.
pub fn clean_excess_sessions() -> usize {
    const MAX_PER_CHANNEL: usize = 30;
    let channels = crate::db::load_all_channel_sessions();
    let mut removed = 0usize;

    for channel in &channels {
        if channel.platform == "tui" {
            continue;
        }

        let mut sessions =
            crate::db::load_claude_sessions_by_channel_id(&channel.id);
        if sessions.len() <= MAX_PER_CHANNEL {
            continue;
        }

        // Sort by created_at descending (newest first)
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Delete sessions beyond the limit
        for cs in sessions.iter().skip(MAX_PER_CHANNEL) {
            let file_id = cs.claude_session_id.as_ref().unwrap_or(&cs.id);
            if let Some(home) = dirs::home_dir() {
                let history_file = home
                    .join(".cc-gateway")
                    .join("history")
                    .join(format!("{}.jsonl", file_id));
                let _ = std::fs::remove_file(&history_file);
            }
            crate::db::delete_claude_session(&cs.id);
            removed += 1;
            info!(
                "Cleaned excess Claude session {} from channel {} (created: {})",
                &cs.id[..cs.id.len().min(8)],
                &channel.id[..channel.id.len().min(8)],
                cs.created_at
            );
        }
    }

    removed
}

pub fn start_background_task(
    log_path: String,
    max_lines: usize,
    max_size_mb: usize,
    media_retention_days: u64,
) {
    let max_lines = if max_lines == 0 { usize::MAX } else { max_lines };
    let max_size_mb = if max_size_mb == 0 { usize::MAX } else { max_size_mb };

    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(tokio::time::Duration::from_secs(8 * 60 * 60));
        ticker.tick().await; // skip the immediate tick

        loop {
            ticker.tick().await;

            // Trim log
            let path = log_path.clone();
            let lines = max_lines;
            let size = max_size_mb;
            match tokio::task::spawn_blocking(move || trim_log_file(&path, lines, size)).await {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => {}
                Ok(Err(e)) => error!("Background log trim failed: {}", e),
                Err(e) => error!("Background log trim task panicked: {}", e),
            }

            // Clean old media files
            if media_retention_days > 0 {
                match tokio::task::spawn_blocking(move || clean_old_media_files(media_retention_days)).await {
                    Ok(Ok(n)) => {
                        if n > 0 {
                            info!("Background media cleanup removed {} files", n);
                        }
                    }
                    Ok(Err(e)) => error!("Background media cleanup failed: {}", e),
                    Err(e) => error!("Background media cleanup panicked: {}", e),
                }
            }

            // Clean up stale TUI sessions
            match tokio::task::spawn_blocking(clean_tui_sessions).await {
                Ok(n) => {
                    if n > 0 {
                        info!("Background TUI session cleanup removed {} sessions", n);
                    }
                }
                Err(e) => error!("Background TUI session cleanup panicked: {}", e),
            }

            // Clean excess sessions for other channels
            match tokio::task::spawn_blocking(clean_excess_sessions).await {
                Ok(n) => {
                    if n > 0 {
                        info!("Background excess session cleanup removed {} sessions", n);
                    }
                }
                Err(e) => error!("Background excess session cleanup panicked: {}", e),
            }
        }
    });
}
