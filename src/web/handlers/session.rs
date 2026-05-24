use axum::{
    extract::{Json, Path, State},
    response::sse::{Event as SseEvent, Sse},
    http::StatusCode,
};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use std::convert::Infallible;
use tokio::sync::Mutex;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tracing::info;

use crate::claude::controller::{ClaudeController, ControllerEvent};
use crate::claude::event_formatter::EventAccumulator;
use crate::config::model::ClaudeConfig;
use crate::session::manager::GLOBAL_SESSIONS;
use crate::web::state::{broadcast_event, EVENT_BUS};

#[derive(Clone)]
pub struct AppState {
    pub claude_config: ClaudeConfig,
    pub show_thinking: bool,
    pub default_dir: String,
}

pub async fn handle_events(Path(session_id): Path<String>) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = EVENT_BUS.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |result| {
        if let Ok(event) = result {
            if event.session_id != session_id {
                return None;
            }
            let json = match serde_json::to_string(&event) {
                Ok(s) => s,
                Err(_) => return None,
            };
            Some(Ok(SseEvent::default().data(json)))
        } else {
            None
        }
    });
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text(""),
    )
}

pub async fn handle_list_sessions() -> Json<serde_json::Value> {
    let sessions = GLOBAL_SESSIONS.list();
    Json(serde_json::json!({
        "sessions": sessions
    }))
}

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    title: Option<String>,
    work_dir: Option<String>,
}

pub async fn handle_create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> (StatusCode, String) {
    let title = req.title.unwrap_or_else(|| "WebUI Session".to_string());
    let work_dir = req.work_dir.unwrap_or_else(|| state.default_dir.clone());
    let expanded = shellexpand::tilde(&work_dir).to_string();

    match GLOBAL_SESSIONS
        .create_webui_session(&title, &expanded, state.claude_config, state.show_thinking)
        .await
    {
        Ok(runtime) => {
            let body = json!({
                "session": runtime.session
            });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to create session: {}", e) });
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    message: String,
}

pub async fn handle_send_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> (StatusCode, String) {
    let runtime = match GLOBAL_SESSIONS.get_webui_runtime(&session_id) {
        Some(r) => r,
        None => {
            match GLOBAL_SESSIONS
                .get_or_create_webui_runtime(&session_id, state.claude_config, state.show_thinking)
                .await
            {
                Some(r) => r,
                None => {
                    let body = json!({ "error": "Session not found" });
                    return (StatusCode::NOT_FOUND, body.to_string());
                }
            }
        }
    };

    let message = req.message.trim().to_string();
    if message.is_empty() {
        let body = json!({ "error": "Empty message" });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    let response = runtime.router.handle(&message).await;

    match response {
        Some(text) => {
            // Immediate response from builtin command
            // Sync session state for /claude and /cd commands
            if message == "/claude" || message.starts_with("/claude ") {
                GLOBAL_SESSIONS.update_active(&session_id, true);
                let ctrl = runtime.controller.lock().await;
                let wd = ctrl.get_work_dir().await;
                let csid = ctrl.get_claude_session_id().await;
                drop(ctrl);
                if !wd.is_empty() {
                    GLOBAL_SESSIONS.update_work_dir(&session_id, &wd);
                }
                if let Some(id) = csid {
                    GLOBAL_SESSIONS.update_claude_session_id(&session_id, Some(&id));
                }
            } else if message.starts_with("/cd ") || message == "/cd_default" {
                let ctrl = runtime.controller.lock().await;
                let wd = ctrl.get_work_dir().await;
                drop(ctrl);
                if !wd.is_empty() {
                    GLOBAL_SESSIONS.update_work_dir(&session_id, &wd);
                }
            }
            broadcast_event(&session_id, "webui", &session_id, "system", &text);
            let body = json!({ "response": text });
            (StatusCode::OK, body.to_string())
        }
        None => {
            // Message forwarded to Claude; record user message first, then spawn poller
            broadcast_event(&session_id, "webui", &session_id, "user", &message);
            tokio::spawn(poll_claude_and_broadcast(
                session_id.clone(),
                runtime.controller.clone(),
            ));
            let body = json!({ "status": "forwarded" });
            (StatusCode::OK, body.to_string())
        }
    }
}

pub async fn handle_stop_session(Path(session_id): Path<String>) -> (StatusCode, String) {
    let runtime = match GLOBAL_SESSIONS.get_webui_runtime(&session_id) {
        Some(r) => r,
        None => {
            let body = json!({ "error": "Session not found" });
            return (StatusCode::NOT_FOUND, body.to_string());
        }
    };

    let ctrl = runtime.controller.lock().await;
    match ctrl.stop_session().await {
        Ok(()) => {
            drop(ctrl);
            GLOBAL_SESSIONS.update_active(&session_id, false);
            let body = json!({ "status": "stopped" });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to stop session: {}", e) });
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_get_history(Path(session_id): Path<String>) -> (StatusCode, String) {
    use std::fs;

    let history_dir = match dirs::home_dir() {
        Some(h) => h.join(".cc-gateway").join("history"),
        None => {
            let body = json!({ "error": "Could not determine home directory" });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    let file_path = history_dir.join(format!("{}.jsonl", session_id));
    if !file_path.exists() {
        let body = json!({ "history": [] });
        return (StatusCode::OK, body.to_string());
    }

    match fs::read_to_string(&file_path) {
        Ok(content) => {
            let mut history = Vec::new();
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(line) {
                    history.push(event);
                }
            }
            let body = json!({ "history": history });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to read history: {}", e) });
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_delete_session(Path(session_id): Path<String>) -> (StatusCode, String) {
    // Stop controller if still active
    if let Some(runtime) = GLOBAL_SESSIONS.get_webui_runtime(&session_id) {
        let ctrl = runtime.controller.lock().await;
        let _ = ctrl.stop_session().await;
    }

    GLOBAL_SESSIONS.remove(&session_id);

    // Delete history file
    let history_dir = match dirs::home_dir() {
        Some(h) => h.join(".cc-gateway").join("history"),
        None => {
            let body = json!({ "status": "deleted", "note": "History cleanup skipped" });
            return (StatusCode::OK, body.to_string());
        }
    };
    let file_path = history_dir.join(format!("{}.jsonl", session_id));
    if file_path.exists() {
        let _ = std::fs::remove_file(&file_path);
    }

    let body = json!({ "status": "deleted" });
    (StatusCode::OK, body.to_string())
}

async fn poll_claude_and_broadcast(
    session_id: String,
    controller: std::sync::Arc<Mutex<ClaudeController>>,
) {
    let event_rx = {
        let ctrl = controller.lock().await;
        ctrl.event_rx_clone()
    };

    let mut accumulator = EventAccumulator::new();
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300);
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut first_text_sent = false;

    info!("[WebUI] Session {} poller started", session_id);

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }

        let event_fut = async {
            let mut rx = event_rx.lock().await;
            rx.recv().await
        };
        tokio::pin!(event_fut);

        tokio::select! {
            _ = interval.tick() => {
                let partial = accumulator.take_output();
                if !partial.trim().is_empty() {
                    broadcast_event(&session_id, "webui", &session_id, "assistant", &partial);
                }
            }
            event_res = tokio::time::timeout(remaining, event_fut) => {
                match event_res {
                    Ok(Some(event)) => {
                        if let ControllerEvent::PermissionRequest { request_id, tool_name, .. } = &event {
                            let card = format!("Permission request: `{}`\nID: `{}`", tool_name, request_id);
                            broadcast_event(&session_id, "webui", &session_id, "system", &card);
                            continue;
                        }
                        let is_text = matches!(event, ControllerEvent::Text(_));
                        let is_done = accumulator.process_event(&event);
                        let should_flush = if !first_text_sent {
                            is_text
                        } else {
                            accumulator.peek_output().len() >= 300
                        };
                        if is_text && should_flush {
                            first_text_sent = true;
                            let partial = accumulator.take_output();
                            if !partial.trim().is_empty() {
                                broadcast_event(&session_id, "webui", &session_id, "assistant", &partial);
                            }
                        }
                        if is_done {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break,
                }
            }
        }
    }

    let reply = accumulator.take_output();
    if !reply.trim().is_empty() {
        broadcast_event(&session_id, "webui", &session_id, "assistant", reply.trim());
    }
}
