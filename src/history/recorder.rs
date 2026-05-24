use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use tracing::{error, info};

use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::state::Event;

/// Start a background task that listens to the global event bus and records
/// WebUI session events to JSONL files in `~/.cc-gateway/history/`.
pub fn start_recorder() {
    tokio::spawn(async {
        let mut rx = crate::web::state::EVENT_BUS.subscribe();
        info!("[History] Recorder started");

        loop {
            match rx.recv().await {
                Ok(event) => {
                    if let Err(e) = record_event(&event).await {
                        error!("[History] Failed to record event: {}", e);
                    }
                }
                Err(e) => {
                    error!("[History] Event bus receive error: {}", e);
                }
            }
        }
    });
}

async fn record_event(event: &Event) -> anyhow::Result<()> {
    // Only record WebUI sessions
    let is_webui = GLOBAL_CHANNEL_SESSIONS
        .get_channel(&event.session_id)
        .map(|c| matches!(c.source, crate::session::channel_model::SessionSource::WebUI))
        .or_else(|| {
            GLOBAL_CHANNEL_SESSIONS
                .get_claude_session(&event.session_id)
                .map(|s| {
                    GLOBAL_CHANNEL_SESSIONS
                        .get_channel(&s.channel_session_id)
                        .map(|c| matches!(c.source, crate::session::channel_model::SessionSource::WebUI))
                        .unwrap_or(false)
                })
        })
        .unwrap_or(false);

    if !is_webui {
        return Ok(());
    }

    let history_dir = get_history_dir()?;
    fs::create_dir_all(&history_dir)?;

    let file_path = history_dir.join(format!("{}.jsonl", event.session_id));
    let line = serde_json::to_string(&serde_json::json!({
        "timestamp": &event.timestamp,
        "role": &event.role,
        "content": &event.content,
    }))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)?;
    writeln!(file, "{}", line)?;

    Ok(())
}

fn get_history_dir() -> anyhow::Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".cc-gateway").join("history"))
}
