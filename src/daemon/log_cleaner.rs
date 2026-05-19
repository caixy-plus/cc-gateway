use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use anyhow::Result;
use tracing::{error, info};

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

pub fn start_background_task(log_path: String, max_lines: usize, max_size_mb: usize) {
    if max_lines == 0 && max_size_mb == 0 {
        return;
    }

    let max_lines = if max_lines == 0 { usize::MAX } else { max_lines };
    let max_size_mb = if max_size_mb == 0 { usize::MAX } else { max_size_mb };

    tokio::spawn(async move {
        let mut ticker =
            tokio::time::interval(tokio::time::Duration::from_secs(8 * 60 * 60));
        ticker.tick().await; // skip the immediate tick

        loop {
            ticker.tick().await;
            let path = log_path.clone();
            let lines = max_lines;
            let size = max_size_mb;

            match tokio::task::spawn_blocking(move || trim_log_file(&path, lines, size)).await {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => {}
                Ok(Err(e)) => error!("Background log trim failed: {}", e),
                Err(e) => error!("Background log trim task panicked: {}", e),
            }
        }
    });
}
