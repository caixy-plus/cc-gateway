use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use tracing::{error, info, warn};

use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::state::Event;

/// Start a background task that listens to the global event bus and records
/// events to JSONL files in `~/.cc-gateway/history/`.
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
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    info!("[History] Event bus closed, shutting down recorder");
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    warn!("[History] Event bus lagged, skipped {} messages", n);
                }
            }
        }
    });
}

async fn record_event(event: &Event) -> anyhow::Result<()> {
    // event.session_id may be:
    // - WebUI: AgentSession.id
    // - Feishu/Telegram: chat_id (ChannelSession.channel_id)
    // Try all three lookup strategies.
    let agent_session =
        // 1) WebUI passes AgentSession.id directly
        GLOBAL_CHANNEL_SESSIONS.get_agent_session(&event.session_id)
        .or_else(|| {
            // 2) session_id might be ChannelSession.id
            GLOBAL_CHANNEL_SESSIONS.get_active_agent_session(&event.session_id)
        })
        .or_else(|| {
            // 3) Feishu/Telegram pass chat_id; find ChannelSession by channel_id
            GLOBAL_CHANNEL_SESSIONS.list_channels()
                .into_iter()
                .find(|c| c.channel_id == event.session_id && c.platform == event.platform)
                .and_then(|c| GLOBAL_CHANNEL_SESSIONS.get_active_agent_session(&c.id))
        });

    let agent_session = match agent_session {
        Some(s) => s,
        None => {
            // No active session for this channel; skip recording.
            return Ok(());
        }
    };

    // Always use the cc-gateway agent session id as the history file name.
    // The provider_session_id can change across resumes and /clear, which
    // would fragment history across multiple files.  The agent_session.id is
    // stable for the lifetime of the session record.
    let history_file_id = &agent_session.id;

    let history_dir = get_history_dir()?;
    fs::create_dir_all(&history_dir)?;

    let file_path = history_dir.join(format!("{}.jsonl", history_file_id));
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
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".cc-gateway").join("history"))
}
