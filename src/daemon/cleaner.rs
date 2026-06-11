use anyhow::Result;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tracing::{error, info, warn};

use crate::config::model::effective_session_retention_per_channel;
use crate::session::channel_model::AgentSession;
use chrono::{DateTime, Utc};

fn clamp_session_retention_per_channel(max_per_channel: usize) -> usize {
    effective_session_retention_per_channel(max_per_channel as u64)
}

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

    let mut output = lines.into_iter().collect::<Vec<_>>().join("\n");
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
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
        info!(
            "Cleaned {} media files older than {} days",
            removed, retention_days
        );
    }

    Ok(removed)
}

const MANAGEMENT_SUBCMDS: &[&str] = &[
    "_daemon", "start", "stop", "restart", "status", "log", "enable", "disable",
];

/// Parse `ps -eo args` output and classify each cc-gateway process.
/// Returns (daemon_count, mgmt_count).
fn classify_cc_gateway_processes() -> (usize, usize) {
    let output = match std::process::Command::new("ps")
        .args(["-eo", "args"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return (0, 0),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
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
        if args.len() >= 2 && args[1] == "_daemon" {
            daemon += 1;
        } else if args.len() >= 2 && MANAGEMENT_SUBCMDS.contains(&args[1]) {
            mgmt += 1;
        }
    }

    (daemon, mgmt)
}

/// Check whether the `cc-gateway` daemon process is currently running.
#[allow(dead_code)]
pub fn is_daemon_running() -> bool {
    let (daemon, _) = classify_cc_gateway_processes();
    daemon > 0
}

/// Last activity time for retention: prefer session `updated_at`, then `created_at`.
fn session_last_updated_at(session: &AgentSession) -> DateTime<Utc> {
    session.updated_at.unwrap_or(session.created_at)
}

fn remove_agent_session_record(session: &AgentSession) {
    crate::history::recorder::delete_session_history(&session.id);
    crate::db::delete_agent_session(&session.id);
}

/// Pick up to `max` sessions per channel: pin the latest-updated per work_dir, then fill by
/// `updated_at` (newest first). When work_dir count exceeds `max`, only the newest `max` pinned
/// sessions survive.
fn select_sessions_to_keep(sessions: &[AgentSession], max: usize) -> HashSet<String> {
    if sessions.len() <= max {
        return sessions.iter().map(|s| s.id.clone()).collect();
    }

    let mut by_time: Vec<&AgentSession> = sessions.iter().collect();
    by_time.sort_by_key(|s| std::cmp::Reverse(session_last_updated_at(s)));

    let mut pinned_by_work_dir: HashMap<&str, &AgentSession> = HashMap::new();
    for session in &by_time {
        pinned_by_work_dir
            .entry(session.work_dir.as_str())
            .or_insert(session);
    }

    let mut keep: Vec<String> = pinned_by_work_dir
        .values()
        .map(|session| session.id.clone())
        .collect();

    if keep.len() > max {
        let mut pinned: Vec<&AgentSession> = pinned_by_work_dir.values().copied().collect();
        pinned.sort_by_key(|s| std::cmp::Reverse(session_last_updated_at(s)));
        keep = pinned
            .into_iter()
            .take(max)
            .map(|session| session.id.clone())
            .collect();
    }

    let mut keep_set: HashSet<String> = keep.into_iter().collect();
    for session in by_time {
        if keep_set.len() >= max {
            break;
        }
        keep_set.insert(session.id.clone());
    }

    keep_set
}

fn prune_agent_sessions(sessions: &[AgentSession], keep: &HashSet<String>) -> usize {
    let mut removed = 0usize;
    for session in sessions {
        if keep.contains(&session.id) {
            continue;
        }
        remove_agent_session_record(session);
        removed += 1;
    }
    removed
}

fn clean_excess_sessions_for_channel(channel_id: &str, max_per_channel: usize) -> usize {
    let sessions = crate::db::load_agent_sessions_by_channel_id(channel_id);
    if sessions.len() <= max_per_channel {
        return 0;
    }

    let keep = select_sessions_to_keep(&sessions, max_per_channel);
    let removed = prune_agent_sessions(&sessions, &keep);
    if removed > 0 {
        info!(
            "Cleaned {} excess Claude sessions from channel {}",
            removed,
            &channel_id[..channel_id.len().min(8)]
        );
    }
    removed
}

/// Per channel: keep at most `max_per_channel` Claude sessions (one newest per work_dir when possible).
/// Returns the number of Claude sessions deleted.
pub fn clean_excess_sessions(max_per_channel: usize) -> usize {
    let max_per_channel = clamp_session_retention_per_channel(max_per_channel);
    let channels = crate::db::load_all_channel_sessions();
    channels
        .iter()
        .map(|channel| clean_excess_sessions_for_channel(&channel.id, max_per_channel))
        .sum()
}

/// Result of one background cleanup cycle (log trim + media + sessions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupCycleResult {
    pub log_trimmed: bool,
    pub media_removed: usize,
    pub excess_sessions_removed: usize,
}

fn normalize_log_limits(max_lines: usize, max_size_mb: usize) -> (usize, usize) {
    let max_lines = if max_lines == 0 {
        usize::MAX
    } else {
        max_lines
    };
    let max_size_mb = if max_size_mb == 0 {
        usize::MAX
    } else {
        max_size_mb
    };
    (max_lines, max_size_mb)
}

/// Run one cleanup cycle: trim logs, purge old media, prune excess sessions.
pub fn run_cleanup_cycle(
    log_path: &str,
    max_lines: usize,
    max_size_mb: usize,
    media_retention_days: u64,
    max_sessions_per_channel: usize,
) -> Result<CleanupCycleResult> {
    let (max_lines, max_size_mb) = normalize_log_limits(max_lines, max_size_mb);
    let log_trimmed = trim_log_file(log_path, max_lines, max_size_mb)?;
    let media_removed = if media_retention_days > 0 {
        clean_old_media_files(media_retention_days)?
    } else {
        0
    };
    let max_sessions_per_channel = clamp_session_retention_per_channel(max_sessions_per_channel);
    let excess_sessions_removed = clean_excess_sessions(max_sessions_per_channel);
    Ok(CleanupCycleResult {
        log_trimmed,
        media_removed,
        excess_sessions_removed,
    })
}

pub fn start_background_task(
    log_path: String,
    max_lines: usize,
    max_size_mb: usize,
    media_retention_days: u64,
    max_sessions_per_channel: usize,
) {
    let (max_lines, max_size_mb) = normalize_log_limits(max_lines, max_size_mb);

    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(tokio::time::Duration::from_secs(8 * 60 * 60));
        ticker.tick().await; // skip the immediate tick

        loop {
            ticker.tick().await;

            let path = log_path.clone();
            let lines = max_lines;
            let size = max_size_mb;
            let retention = media_retention_days;
            let session_cap = max_sessions_per_channel;
            match tokio::task::spawn_blocking(move || {
                run_cleanup_cycle(&path, lines, size, retention, session_cap)
            })
            .await
            {
                Ok(Ok(result)) => {
                    if result.log_trimmed {
                        info!("Background log trim completed");
                    }
                    if result.media_removed > 0 {
                        info!(
                            "Background media cleanup removed {} files",
                            result.media_removed
                        );
                    }
                    if result.excess_sessions_removed > 0 {
                        info!(
                            "Background excess session cleanup removed {} sessions",
                            result.excess_sessions_removed
                        );
                    }
                }
                Ok(Err(e)) => error!("Background cleanup cycle failed: {}", e),
                Err(e) => error!("Background cleanup cycle panicked: {}", e),
            }
        }
    });
}
