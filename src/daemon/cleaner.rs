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
    let mut lines = VecDeque::with_capacity(max_lines);
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
    std::fs::write(&path, output)?;

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
        }
    });
}
