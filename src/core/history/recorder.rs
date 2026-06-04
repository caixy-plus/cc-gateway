use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use chrono::Utc;
use tracing::{error, info, warn};

use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::state::Event;

/// Append one line to `~/.cc-gateway/history/{agent_session_id}.jsonl`.
///
/// Safe to call from any process (daemon, tests). WebUI reads these files via
/// `GET /api/sessions/:id/history`.
pub fn append_session_history(
    agent_session_id: &str,
    role: &str,
    content: &str,
) -> anyhow::Result<()> {
    if agent_session_id.is_empty() || content.trim().is_empty() {
        return Ok(());
    }

    let history_dir = get_history_dir()?;
    fs::create_dir_all(&history_dir)?;

    let file_path = history_dir.join(format!("{}.jsonl", agent_session_id));
    let line = serde_json::to_string(&serde_json::json!({
        "timestamp": Utc::now().to_rfc3339(),
        "role": role,
        "content": content,
    }))?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)?;
    writeln!(file, "{}", line)?;

    Ok(())
}

/// Merge adjacent streaming chunks (same `role`) into one event for display.
///
/// Bot channels may append one JSONL line per poller flush; WebUI maps
/// each line to a separate chat bubble unless coalesced here.
pub fn coalesce_streaming_history(events: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = Vec::new();
    for event in events {
        let role = event
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let content = event
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if content.is_empty() {
            continue;
        }
        let mergeable = role == "assistant" || role == "thinking";
        if mergeable {
            if let Some(last) = out.last_mut() {
                let last_role = last.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if last_role == role {
                    let prev = last
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(obj) = last.as_object_mut() {
                        obj.insert(
                            "content".to_string(),
                            serde_json::Value::String(format!("{prev}{content}")),
                        );
                    }
                    continue;
                }
            }
        }
        out.push(event);
    }
    out
}

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

fn resolve_agent_session_for_event(event: &Event) -> Option<crate::session::channel_model::AgentSession> {
    // event.session_id may be:
    // - WebUI: AgentSession.id
    // - Feishu/Telegram/QQ: chat_id (ChannelSession.channel_id)
    // Try all lookup strategies.
    if let Some(s) = GLOBAL_CHANNEL_SESSIONS.get_agent_session(&event.session_id) {
        return Some(s);
    }
    if let Some(s) = GLOBAL_CHANNEL_SESSIONS.get_active_agent_session(&event.session_id) {
        return Some(s);
    }
    let channel = GLOBAL_CHANNEL_SESSIONS.list_channels().into_iter().find(|c| {
        c.channel_id == event.session_id && c.platform == event.platform
    })?;
    GLOBAL_CHANNEL_SESSIONS
        .get_active_agent_session(&channel.id)
        .or_else(|| {
            // Bot platforms broadcast user messages before auto-resume creates an
            // active runtime; attach to the latest session record for this channel.
            GLOBAL_CHANNEL_SESSIONS
                .list_agent_sessions_by_channel(&channel.id, Some(1))
                .into_iter()
                .next()
        })
}

async fn record_event(event: &Event) -> anyhow::Result<()> {
    let agent_session = match resolve_agent_session_for_event(event) {
        Some(s) => s,
        None => return Ok(()),
    };

    // Always use the cc-gateway agent session id as the history file name.
    // The provider_session_id can change across resumes and /clear, which
    // would fragment history across multiple files.  The agent_session.id is
    // stable for the lifetime of the session record.
    append_session_history(&agent_session.id, &event.role, &event.content)
}

fn get_history_dir() -> anyhow::Result<PathBuf> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;
    Ok(home.join(".cc-gateway").join("history"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coalesce_streaming_history_merges_adjacent_assistant_chunks() {
        let events = vec![
            serde_json::json!({"role": "user", "content": "hi"}),
            serde_json::json!({"role": "assistant", "content": "2 + "}),
            serde_json::json!({"role": "assistant", "content": "4 = 6"}),
        ];
        let merged = coalesce_streaming_history(events);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[1]["content"], "2 + 4 = 6");
    }

    #[tokio::test]
    async fn resolve_agent_session_falls_back_to_latest_inactive_channel_session() {
        use crate::db;
        use crate::tests::helpers::TestEnv;

        // Acquire the shared ENV_LOCK and an isolated temp HOME via TestEnv so this
        // test serializes with every other HOME-mutating test. Raw `set_var("HOME")`
        // here previously raced the config/db tests under parallel `cargo test`.
        let _env = TestEnv::new();

        GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
        db::init_schema().unwrap();

        let channel = GLOBAL_CHANNEL_SESSIONS
            .get_or_create_platform_channel("feishu", "chat-fallback", "/tmp")
            .await;
        let session = GLOBAL_CHANNEL_SESSIONS
            .create_agent_session_only(&channel.id, "t", "/tmp", "claude")
            .unwrap();

        let event = crate::web::state::Event {
            session_id: "chat-fallback".to_string(),
            platform: "feishu".to_string(),
            chat_id: "chat-fallback".to_string(),
            role: "user".to_string(),
            content: "hi".to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };
        let resolved = resolve_agent_session_for_event(&event);
        assert_eq!(resolved.as_ref().map(|s| s.id.as_str()), Some(session.id.as_str()));

        GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
        // HOME restoration + temp-dir cleanup handled by TestEnv::drop.
    }

    #[test]
    fn append_session_history_writes_jsonl_line() {
        let id = format!("test-history-{}", std::process::id());
        append_session_history(&id, "user", "hello").expect("append");
        let path = match get_history_dir() {
            Ok(dir) => dir.join(format!("{}.jsonl", id)),
            Err(_) => return,
        };
        if !path.exists() {
            return;
        }
        let body = std::fs::read_to_string(&path).expect("read");
        assert!(body.contains("\"role\":\"user\"") || body.contains("\"role\": \"user\""));
        assert!(body.contains("hello"));
        let _ = std::fs::remove_file(path);
    }
}
