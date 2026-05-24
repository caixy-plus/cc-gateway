use axum::{
    extract::{Json, Path, Query, State},
    response::sse::{Event as SseEvent, Sse},
    http::StatusCode,
};
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tracing::info;

use crate::claude::controller::ClaudeController;
use crate::claude::event_poller::{ClaudeEventPoller, EventPollSink};
use crate::command::{CommandAction, CommandRouter};
use crate::config::model::ClaudeConfig;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::state::{broadcast_event, EVENT_BUS};

#[derive(Clone)]
pub struct AppState {
    pub claude_config: ClaudeConfig,
    pub show_thinking: bool,
    pub default_dir: String,
}

async fn ensure_webui_channel(default_dir: &str) -> anyhow::Result<String> {
    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", default_dir)
        .await?;
    Ok(runtime.channel_session.id.clone())
}

pub async fn handle_events() -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let rx = EVENT_BUS.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        if let Ok(event) = result {
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

#[derive(Deserialize)]
pub struct ListSessionsQuery {
    source: Option<String>,
}

pub async fn handle_list_sessions(Query(query): Query<ListSessionsQuery>) -> Json<serde_json::Value> {
    let sessions = GLOBAL_CHANNEL_SESSIONS.list_claude_sessions();
    let channels: HashMap<String, crate::session::channel_model::ChannelSession> =
        GLOBAL_CHANNEL_SESSIONS
            .list_channels()
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();

    let source_filter = query.source.unwrap_or_else(|| "all".to_string());

    let mapped: Vec<serde_json::Value> = sessions
        .into_iter()
        .filter(|s| {
            if source_filter == "all" {
                return true;
            }
            channels.get(&s.channel_session_id)
                .map(|c| format!("{:?}", c.source) == source_filter)
                .unwrap_or(false)
        })
        .map(|s| {
            let channel = channels.get(&s.channel_session_id);
            serde_json::json!({
                "id": s.id,
                "title": s.title,
                "source": channel.map(|c| c.source.clone()).unwrap_or(crate::session::channel_model::SessionSource::WebUI),
                "platform": channel.map(|c| c.platform.clone()).unwrap_or_else(|| "webui".to_string()),
                "chat_id": s.channel_session_id,
                "work_dir": channel.map(|c| c.work_dir.clone()).unwrap_or_else(|| s.work_dir.clone()),
                "active": s.active,
                "claude_session_id": s.claude_session_id,
                "created_at": s.created_at,
            })
        })
        .collect();

    Json(serde_json::json!({
        "sessions": mapped
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
    let channel_id = match ensure_webui_channel(&state.default_dir).await {
        Ok(id) => id,
        Err(e) => {
            let body = json!({ "error": format!("Failed to ensure WebUI channel: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };
    let title = req.title.unwrap_or_else(|| "WebUI Session".to_string());
    let work_dir = req.work_dir.unwrap_or_else(|| state.default_dir.clone());
    let expanded = shellexpand::tilde(&work_dir).to_string();

    match GLOBAL_CHANNEL_SESSIONS.create_claude_session_only(&channel_id, &title, &expanded) {
        Ok(session) => {
            let channel = GLOBAL_CHANNEL_SESSIONS.get_channel(&channel_id);
            let body = json!({
                "session": {
                    "id": session.id,
                    "title": session.title,
                    "source": channel.as_ref().map(|c| c.source.clone()).unwrap_or(crate::session::channel_model::SessionSource::WebUI),
                    "platform": channel.as_ref().map(|c| c.platform.clone()).unwrap_or_else(|| "webui".to_string()),
                    "chat_id": session.channel_session_id,
                    "work_dir": session.work_dir,
                    "active": session.active,
                    "claude_session_id": session.claude_session_id,
                    "created_at": session.created_at,
                }
            });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to create session: {}", e) });
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_start_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> (StatusCode, String) {
    let channel_id = match ensure_webui_channel(&state.default_dir).await {
        Ok(id) => id,
        Err(e) => {
            let body = json!({ "error": format!("Failed to ensure WebUI channel: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    // If this session is already the active one, nothing to do
    if let Some(active) = GLOBAL_CHANNEL_SESSIONS.get_active_claude_session(&channel_id) {
        if active.id == session_id {
            let body = json!({ "status": "already_active" });
            return (StatusCode::OK, body.to_string());
        }
    }

    let claude_config = state.claude_config.clone();
    match GLOBAL_CHANNEL_SESSIONS
        .resume_claude_session(&session_id, claude_config, state.show_thinking)
        .await
    {
        Ok((session, controller)) => {
            let router = std::sync::Arc::new(CommandRouter::new(controller.clone(), &state.default_dir));
            let active = crate::session::channel_manager::ActiveClaudeRuntime {
                claude_session: session.clone(),
                controller: controller.clone(),
                router,
            };
            GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&channel_id, Some(active));

            let channel = GLOBAL_CHANNEL_SESSIONS.get_channel(&channel_id);
            let body = json!({
                "status": "started",
                "session": {
                    "id": session.id,
                    "title": session.title,
                    "source": channel.as_ref().map(|c| c.source.clone()).unwrap_or(crate::session::channel_model::SessionSource::WebUI),
                    "platform": channel.as_ref().map(|c| c.platform.clone()).unwrap_or_else(|| "webui".to_string()),
                    "chat_id": session.channel_session_id,
                    "work_dir": session.work_dir,
                    "active": session.active,
                    "claude_session_id": session.claude_session_id,
                    "created_at": session.created_at,
                }
            });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to start session: {}", e) });
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
    let channel_id = match ensure_webui_channel(&state.default_dir).await {
        Ok(id) => id,
        Err(e) => {
            let body = json!({ "error": format!("Failed to ensure WebUI channel: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };
    let message = req.message.trim().to_string();
    if message.is_empty() {
        let body = json!({ "error": "Empty message" });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    // Check if the requested session is the currently active one
    let active_session = GLOBAL_CHANNEL_SESSIONS.get_active_claude_session(&channel_id);
    let needs_switch = match &active_session {
        Some(s) if s.id == session_id => false,
        _ => true,
    };

    if needs_switch {
        let claude_config = state.claude_config.clone();
        // Try to resume the requested session
        match GLOBAL_CHANNEL_SESSIONS
            .resume_claude_session(&session_id, claude_config.clone(), state.show_thinking)
            .await
        {
            Ok((session, controller)) => {
                let router = std::sync::Arc::new(CommandRouter::new(controller.clone(), &state.default_dir));
                let active = crate::session::channel_manager::ActiveClaudeRuntime {
                    claude_session: session.clone(),
                    controller: controller.clone(),
                    router,
                };
                GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&channel_id, Some(active));
            }
            Err(e) => {
                let body = json!({ "error": format!("Session not active and could not be resumed: {}", e) });
                return (StatusCode::NOT_FOUND, body.to_string());
            }
        }
    }

    let runtime = match GLOBAL_CHANNEL_SESSIONS.get_webui_runtime(&channel_id) {
        Some(r) => r,
        None => {
            let body = json!({ "error": "WebUI runtime not found" });
            return (StatusCode::NOT_FOUND, body.to_string());
        }
    };

    let mut active = match runtime.active_claude {
        Some(a) => a,
        None => {
            let body = json!({ "error": "No active session" });
            return (StatusCode::NOT_FOUND, body.to_string());
        }
    };

    // Guard: if the controller's session died since we last checked, try to restart it.
    {
        let ctrl = active.controller.lock().await;
        if !ctrl.is_session_active().await {
            drop(ctrl);
            let claude_config = state.claude_config.clone();
            match GLOBAL_CHANNEL_SESSIONS
                .resume_claude_session(&session_id, claude_config, state.show_thinking)
                .await
            {
                Ok((session, controller)) => {
                    let router = std::sync::Arc::new(CommandRouter::new(controller.clone(), &state.default_dir));
                    let new_active = crate::session::channel_manager::ActiveClaudeRuntime {
                        claude_session: session.clone(),
                        controller: controller.clone(),
                        router,
                    };
                    GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&channel_id, Some(new_active.clone()));
                    active = new_active;
                }
                Err(e) => {
                    let body = json!({ "error": format!("Session died and could not be restarted: {}", e) });
                    return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
                }
            }
        }
    }

    let action = active.router.route(&message).await;

    match action {
        CommandAction::ForwardToClaude(text) => {
            let response = active.router.execute(CommandAction::ForwardToClaude(text.clone())).await;
            if let Some(reply) = response {
                broadcast_event(&session_id, "webui", &session_id, "system", &reply);
                let body = json!({ "response": reply });
                return (StatusCode::OK, body.to_string());
            }
            broadcast_event(&session_id, "webui", &session_id, "user", &text);
            GLOBAL_CHANNEL_SESSIONS.touch_claude_session(&session_id);
            // Cancel any previous poller before spawning a new one to prevent
            // concurrent pollers from splitting the event stream between them.
            GLOBAL_CHANNEL_SESSIONS.set_webui_poll_handle(&channel_id, None);
            let join_handle = tokio::spawn(poll_claude_and_broadcast(
                session_id.clone(),
                active.controller.clone(),
            ));
            GLOBAL_CHANNEL_SESSIONS.set_webui_poll_handle(&channel_id, Some(join_handle.abort_handle()));
            let body = json!({ "status": "forwarded" });
            (StatusCode::OK, body.to_string())
        }
        CommandAction::StopSession => {
            // stop_channel_session internally calls ctrl.stop_session(),
            // aborts the poller, and marks the session inactive.
            match GLOBAL_CHANNEL_SESSIONS.stop_channel_session(&channel_id).await {
                Ok(()) => {
                    broadcast_event(&session_id, "webui", &session_id, "system", "Session stopped.");
                    let body = json!({ "status": "stopped", "session_id": session_id });
                    (StatusCode::OK, body.to_string())
                }
                Err(e) => {
                    let body = json!({ "error": format!("Failed to stop session: {}", e) });
                    (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
                }
            }
        }
        CommandAction::ChangeDir(_) | CommandAction::ChangeDirDefault => {
            let response = active.router.execute(action).await;
            if let Some(text) = response {
                let ctrl = active.controller.lock().await;
                let wd = ctrl.get_work_dir().await;
                drop(ctrl);
                if !wd.is_empty() {
                    let _ = GLOBAL_CHANNEL_SESSIONS.switch_work_dir(&channel_id, PathBuf::from(wd)).await;
                }
                broadcast_event(&session_id, "webui", &session_id, "system", &text);
                let body = json!({ "response": text });
                return (StatusCode::OK, body.to_string());
            }
            let body = json!({ "response": "" });
            (StatusCode::OK, body.to_string())
        }
        _ => {
            let response = active.router.execute(action).await;
            if let Some(text) = response {
                broadcast_event(&session_id, "webui", &session_id, "system", &text);
                let body = json!({ "response": text });
                return (StatusCode::OK, body.to_string());
            }
            let body = json!({ "response": "" });
            (StatusCode::OK, body.to_string())
        }
    }
}

pub async fn handle_stop_session(Path(session_id): Path<String>) -> (StatusCode, String) {
    let channel_id = match GLOBAL_CHANNEL_SESSIONS
        .get_claude_session(&session_id)
        .map(|s| s.channel_session_id.clone())
    {
        Some(id) => id,
        None => {
            let body = json!({ "error": "Session not found" });
            return (StatusCode::NOT_FOUND, body.to_string());
        }
    };

    match GLOBAL_CHANNEL_SESSIONS.stop_channel_session(&channel_id).await {
        Ok(()) => {
            let body = json!({ "status": "stopped", "session_id": session_id });
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

    // Use claude_session_id if available, otherwise fall back to session_id
    let file_id = GLOBAL_CHANNEL_SESSIONS
        .get_claude_session(&session_id)
        .and_then(|s| s.claude_session_id)
        .unwrap_or(session_id);

    let file_path = history_dir.join(format!("{}.jsonl", file_id));
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
    let channel_id = GLOBAL_CHANNEL_SESSIONS
        .get_claude_session(&session_id)
        .map(|s| s.channel_session_id.clone());

    // Stop controller if this session is active
    if let Some(ref cid) = channel_id {
        if let Some(runtime) = GLOBAL_CHANNEL_SESSIONS.get_webui_runtime(cid) {
            if let Some(ref active) = runtime.active_claude {
                if active.claude_session.id == session_id {
                    let _ = GLOBAL_CHANNEL_SESSIONS.stop_channel_session(cid).await;
                }
            }
        }
    }

    // Get claude_session_id BEFORE removing the session so we can delete the correct history file.
    let file_id = GLOBAL_CHANNEL_SESSIONS
        .get_claude_session(&session_id)
        .and_then(|s| s.claude_session_id)
        .unwrap_or_else(|| session_id.clone());

    GLOBAL_CHANNEL_SESSIONS.remove_claude_session(&session_id);

    // Delete history file
    let history_dir = match dirs::home_dir() {
        Some(h) => h.join(".cc-gateway").join("history"),
        None => {
            let body = json!({ "status": "deleted", "note": "History cleanup skipped" });
            return (StatusCode::OK, body.to_string());
        }
    };

    let file_path = history_dir.join(format!("{}.jsonl", file_id));
    if file_path.exists() {
        let _ = std::fs::remove_file(&file_path);
    }

    let body = json!({ "status": "deleted" });
    (StatusCode::OK, body.to_string())
}

// ---------------------------------------------------------------------------
// Claude event poller for WebUI
// ---------------------------------------------------------------------------

struct WebUIEventSink {
    session_id: String,
}

#[async_trait::async_trait]
impl EventPollSink for WebUIEventSink {
    async fn flush(&mut self, text: &str, _is_done: bool) -> anyhow::Result<()> {
        broadcast_event(&self.session_id, "webui", &self.session_id, "assistant", text);
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        _input: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        let card = format!("Permission request: `{}`\nID: `{}`", tool_name, request_id);
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &card);
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> anyhow::Result<()> {
        let text = format!("Confirm: {} (id: {})\nOptions: {:?}\n", prompt, request_id, options);
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &text);
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> anyhow::Result<()> {
        let text = format!("Select: {} (id: {})\nOptions: {:?}\n", prompt, request_id, options);
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &text);
        Ok(())
    }

    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::claude::controller::QuestionItem],
    ) -> anyhow::Result<()> {
        let mut text = format!("Question (id: {})\n", request_id);
        for q in questions {
            text.push_str(&format!("  {}: {}\n", q.header, q.question));
            for opt in &q.options {
                text.push_str(&format!("    - {}: {}\n", opt.label, opt.description));
            }
        }
        text.push('\n');
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &text);
        Ok(())
    }

}

async fn poll_claude_and_broadcast(
    session_id: String,
    controller: std::sync::Arc<Mutex<ClaudeController>>,
) {
    info!("[WebUI] Session {} poller started", session_id);

    let poller = {
        let ctrl = controller.lock().await;
        ClaudeEventPoller::from_controller(&*ctrl)
    };

    let mut sink = WebUIEventSink { session_id };
    if let Err(e) = poller.run(&mut sink).await {
        tracing::warn!("[WebUI] Poller error: {}", e);
    }
}
