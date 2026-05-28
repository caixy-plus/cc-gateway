use axum::{
    extract::{Json, Path, Query, State},
    http::StatusCode,
    response::sse::{Event as SseEvent, Sse},
};
use chrono::FixedOffset;
use futures::stream::Stream;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use tokio::sync::Mutex;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tracing::info;

use crate::command::CommandAction;
use crate::config::model::AgentProfiles;
use crate::runtime::controller::AgentController;
use crate::runtime::event_poller::{AgentEventPoller, EventPollSink};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::state::{broadcast_event, EVENT_BUS};

#[derive(Clone)]
pub struct AppState {
    pub agent_settings: AgentProfiles,
    pub show_thinking: bool,
    pub default_dir: String,
    pub daemon_config_path: Option<PathBuf>,
}

// ---------------------------------------------------------------------------
// Output buffering policy (WebUI / SSE)
// ---------------------------------------------------------------------------

const WEBUI_FLUSH_INTERVAL_MS: u64 = 100;
const WEBUI_MAX_BUFFER_CHARS: usize = 2000;

async fn ensure_webui_channel(default_dir: &str) -> anyhow::Result<String> {
    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", default_dir)
        .await?;
    Ok(runtime.channel_session.id.clone())
}

fn json_error(key: &str, msg: impl Into<String>) -> serde_json::Value {
    json!({ "error_key": key, "error": msg.into() })
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
    pub(crate) source: Option<String>,
}

pub async fn handle_list_sessions(
    Query(query): Query<ListSessionsQuery>,
) -> Json<serde_json::Value> {
    let sessions = GLOBAL_CHANNEL_SESSIONS.list_agent_sessions();
    let channels: HashMap<String, crate::session::channel_model::ChannelSession> =
        GLOBAL_CHANNEL_SESSIONS
            .list_channels()
            .into_iter()
            .map(|c| (c.id.clone(), c))
            .collect();

    let source_filter = query.source.unwrap_or_else(|| "webui".to_string());

    let beijing_offset = FixedOffset::east_opt(8 * 3600).unwrap();

    let mapped: Vec<serde_json::Value> = sessions
        .into_iter()
        .filter(|s| {
            if source_filter == "all" {
                return true;
            }
            channels.get(&s.channel_session_id)
                .map(|c| c.source.to_string().eq_ignore_ascii_case(&source_filter))
                .unwrap_or(false)
        })
        .map(|s| {
            let channel = channels.get(&s.channel_session_id);
            let created_at_local = s.created_at.with_timezone(&beijing_offset);
            let stopped_at_local = s.stopped_at.map(|t| t.with_timezone(&beijing_offset));
            let updated_at_local = s.updated_at.map(|t| t.with_timezone(&beijing_offset));
            serde_json::json!({
                "id": s.id,
                "title": s.title,
                "source": channel.map(|c| c.source.to_string()).unwrap_or_else(|| "webui".to_string()),
                "platform": channel.map(|c| c.platform.clone()).unwrap_or_else(|| "webui".to_string()),
                "chat_id": s.channel_session_id,
                "work_dir": s.work_dir,
                "active": s.active,
                "provider": s.provider,
                "provider_session_id": s.provider_session_id,
                "created_at": created_at_local.to_rfc3339(),
                "stopped_at": stopped_at_local.map(|t| t.to_rfc3339()),
                "updated_at": updated_at_local.map(|t| t.to_rfc3339()),
            })
        })
        .collect();

    Json(serde_json::json!({
        "sessions": mapped
    }))
}

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub(crate) title: Option<String>,
    #[serde(alias = "workDir")]
    pub(crate) work_dir: Option<String>,
}

pub async fn handle_create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> (StatusCode, String) {
    let channel_id = match ensure_webui_channel(&state.default_dir).await {
        Ok(id) => id,
        Err(e) => {
            let body = json_error(
                "webui.runtime_not_found",
                format!("Failed to ensure WebUI channel: {}", e),
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };
    let title = req.title.unwrap_or_else(|| "WebUI Session".to_string());
    let work_dir = req
        .work_dir
        .filter(|dir| dir.trim() != "~")
        .unwrap_or_else(|| state.default_dir.clone());
    let expanded = shellexpand::tilde(&work_dir).to_string();

    let default_provider = state.agent_settings.default.to_string();
    match GLOBAL_CHANNEL_SESSIONS.create_agent_session_only(
        &channel_id,
        &title,
        &expanded,
        &default_provider,
    ) {
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
                    "provider": session.provider,
                    "provider_session_id": session.provider_session_id,
                    "created_at": session.created_at,
                }
            });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json_error(
                "webui.runtime_not_found",
                format!("Failed to create session: {}", e),
            );
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
            let body = json_error(
                "webui.runtime_not_found",
                format!("Failed to ensure WebUI channel: {}", e),
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    // WebUI supports multiple sessions running concurrently under one channel.
    // If this session is already running, nothing to do.
    if let Some(active) = GLOBAL_CHANNEL_SESSIONS.get_webui_active_agent(&channel_id, &session_id) {
        let ctrl = active.controller.lock().await;
        if ctrl.is_session_active().await {
            let body = json!({ "status": "already_active" });
            return (StatusCode::OK, body.to_string());
        }
    }

    match GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_runtime(
            &session_id,
            &state.default_dir,
            state.agent_settings.clone(),
            state.show_thinking,
        )
        .await
    {
        Ok(active) => {
            let session = active.agent_session.clone();
            let controller = active.controller.clone();
            GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&channel_id, active);

            // Start long-running poller for this session
            if !GLOBAL_CHANNEL_SESSIONS
                .has_webui_poll_handle(&channel_id, &session.id)
                .await
            {
                let abort_handle = spawn_webui_poller_task(
                    channel_id.clone(),
                    session.id.clone(),
                    controller.clone(),
                );
                GLOBAL_CHANNEL_SESSIONS.set_webui_poll_handle(
                    &channel_id,
                    &session.id,
                    abort_handle,
                );
            }

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
                    "provider": session.provider,
                    "provider_session_id": session.provider_session_id,
                    "created_at": session.created_at,
                }
            });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json_error(
                "webui.runtime_not_found",
                format!("Failed to start session: {}", e),
            );
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub(crate) message: String,
}

pub async fn handle_send_message(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> (StatusCode, String) {
    let channel_id = match ensure_webui_channel(&state.default_dir).await {
        Ok(id) => id,
        Err(e) => {
            let body = json_error(
                "webui.runtime_not_found",
                format!("Failed to ensure WebUI channel: {}", e),
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };
    let message = req.message.trim().to_string();
    if message.is_empty() {
        let body = json_error("webui.empty_message", crate::t!("webui.empty_message"));
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    // Ensure the session runtime is active (WebUI supports multiple active sessions).
    let mut active = match GLOBAL_CHANNEL_SESSIONS.get_webui_active_agent(&channel_id, &session_id)
    {
        Some(a) => a,
        None => {
            match GLOBAL_CHANNEL_SESSIONS
                .resume_agent_session_runtime(
                    &session_id,
                    &state.default_dir,
                    state.agent_settings.clone(),
                    state.show_thinking,
                )
                .await
            {
                Ok(active) => {
                    let session = active.agent_session.clone();
                    let controller = active.controller.clone();
                    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&channel_id, active.clone());
                    if !GLOBAL_CHANNEL_SESSIONS
                        .has_webui_poll_handle(&channel_id, &session.id)
                        .await
                    {
                        let abort_handle = spawn_webui_poller_task(
                            channel_id.clone(),
                            session.id.clone(),
                            controller.clone(),
                        );
                        GLOBAL_CHANNEL_SESSIONS.set_webui_poll_handle(
                            &channel_id,
                            &session.id,
                            abort_handle,
                        );
                    }
                    active
                }
                Err(e) => {
                    let body = json_error(
                        "webui.session_not_found",
                        format!("Session not active and could not be resumed: {}", e),
                    );
                    return (StatusCode::NOT_FOUND, body.to_string());
                }
            }
        }
    };

    // Guard: if the controller's session died since we last checked, try to restart it.
    {
        let ctrl = active.controller.lock().await;
        if !ctrl.is_session_active().await {
            drop(ctrl);
            let _ = GLOBAL_CHANNEL_SESSIONS
                .stop_webui_session(&channel_id, &session_id)
                .await;

            match GLOBAL_CHANNEL_SESSIONS
                .resume_agent_session_runtime(
                    &session_id,
                    &state.default_dir,
                    state.agent_settings.clone(),
                    state.show_thinking,
                )
                .await
            {
                Ok(new_active) => {
                    let session = new_active.agent_session.clone();
                    let controller = new_active.controller.clone();
                    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&channel_id, new_active.clone());
                    active = new_active;

                    // Start long-running poller for the restarted session
                    if !GLOBAL_CHANNEL_SESSIONS
                        .has_webui_poll_handle(&channel_id, &session.id)
                        .await
                    {
                        let abort_handle = spawn_webui_poller_task(
                            channel_id.clone(),
                            session.id.clone(),
                            controller.clone(),
                        );
                        GLOBAL_CHANNEL_SESSIONS.set_webui_poll_handle(
                            &channel_id,
                            &session.id,
                            abort_handle,
                        );
                    }
                }
                Err(e) => {
                    let body = json_error(
                        "webui.failed_stop_session",
                        format!("Session died and could not be restarted: {}", e),
                    );
                    return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
                }
            }
        }
    }

    ensure_webui_poller_task(&channel_id, &session_id, active.controller.clone()).await;

    let action = active.router.route(&message).await;

    match action {
        CommandAction::ForwardToAgent(text) => {
            let response = active
                .router
                .execute(CommandAction::ForwardToAgent(text.clone()))
                .await;
            if let Some(reply) = response {
                broadcast_event(&session_id, "webui", &session_id, "system", &reply);
                let body = json!({ "response": reply });
                return (StatusCode::OK, body.to_string());
            }
            broadcast_event(&session_id, "webui", &session_id, "user", &text);
            GLOBAL_CHANNEL_SESSIONS.touch_agent_session(&session_id);
            // Poller is already running (started in handle_start_session or resumed above)
            let body = json!({ "status": "forwarded" });
            (StatusCode::OK, body.to_string())
        }
        CommandAction::StopSession => {
            let stopped_provider = active.agent_session.stored_provider();
            // Gracefully stop controller first, then mark session inactive.
            {
                let ctrl = active.controller.lock().await;
                let _ = ctrl.stop_session().await;
            }
            match GLOBAL_CHANNEL_SESSIONS
                .stop_channel_session(&channel_id)
                .await
            {
                Ok(()) => {
                    broadcast_event(
                        &session_id,
                        "webui",
                        &session_id,
                        "system",
                        &crate::command::agents::session_stopped_message(&stopped_provider),
                    );
                    let body = json!({ "status": "stopped", "session_id": session_id });
                    (StatusCode::OK, body.to_string())
                }
                Err(e) => {
                    let body = json_error(
                        "webui.failed_stop_session",
                        crate::t_fmt!("webui.failed_stop_session", ERR = e),
                    );
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
                    let _ = GLOBAL_CHANNEL_SESSIONS
                        .switch_work_dir(&channel_id, PathBuf::from(wd))
                        .await;
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

async fn ensure_webui_poller_task(
    channel_id: &str,
    session_id: &str,
    controller: std::sync::Arc<Mutex<AgentController>>,
) {
    if GLOBAL_CHANNEL_SESSIONS
        .has_webui_poll_handle(channel_id, session_id)
        .await
    {
        return;
    }
    let abort_handle =
        spawn_webui_poller_task(channel_id.to_string(), session_id.to_string(), controller);
    GLOBAL_CHANNEL_SESSIONS.set_webui_poll_handle(channel_id, session_id, abort_handle);
}

pub async fn handle_stop_session(Path(session_id): Path<String>) -> (StatusCode, String) {
    let channel_id = match GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .map(|s| s.channel_session_id.clone())
    {
        Some(id) => id,
        None => {
            let body = json_error(
                "webui.session_not_found",
                crate::t!("webui.session_not_found"),
            );
            return (StatusCode::NOT_FOUND, body.to_string());
        }
    };

    match GLOBAL_CHANNEL_SESSIONS
        .stop_webui_session(&channel_id, &session_id)
        .await
    {
        Ok(()) => {
            let body = json!({ "status": "stopped", "session_id": session_id });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json_error(
                "webui.failed_stop_session",
                format!("Failed to stop session: {}", e),
            );
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_get_history(Path(session_id): Path<String>) -> (StatusCode, String) {
    use std::fs;

    let history_dir = match dirs::home_dir() {
        Some(h) => h.join(".cc-gateway").join("history"),
        None => {
            let body = json_error("webui.home_dir_error", crate::t!("webui.home_dir_error"));
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    // Use provider_session_id as history file name when available.
    let file_id = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .and_then(|s| s.provider_session_id)
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
            let body = json_error(
                "webui.session_not_found",
                format!("Failed to read history: {}", e),
            );
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_delete_session(Path(session_id): Path<String>) -> (StatusCode, String) {
    if GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .map(|s| s.active)
        .unwrap_or(false)
    {
        let body = json_error(
            "webui.cannot_delete_active",
            crate::t!("webui.cannot_delete_active"),
        );
        return (StatusCode::CONFLICT, body.to_string());
    }

    // Get provider session id BEFORE removing the session so we can delete the correct history file.
    let file_id = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .and_then(|s| s.provider_session_id)
        .unwrap_or_else(|| session_id.clone());

    if !GLOBAL_CHANNEL_SESSIONS.remove_agent_session(&session_id) {
        let body = json_error(
            "webui.cannot_delete_active",
            crate::t!("webui.cannot_delete_active"),
        );
        return (StatusCode::CONFLICT, body.to_string());
    }

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
        if text.trim().is_empty() {
            return Ok(());
        }
        broadcast_event(
            &self.session_id,
            "webui",
            &self.session_id,
            "assistant",
            text,
        );
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        input: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        let mut card = crate::t_fmt!(
            "webui.permission_request",
            NAME = tool_name,
            ID = request_id
        );
        if let Some(input) = input {
            let pretty = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
            card.push_str("\n\n");
            card.push_str(crate::t!("webui.permission_request_input"));
            card.push('\n');
            card.push_str("```json\n");
            card.push_str(&pretty);
            card.push_str("\n```");
        }
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &card);
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> anyhow::Result<()> {
        let text = crate::t_fmt!(
            "webui.confirm_request",
            PROMPT = prompt,
            ID = request_id,
            OPTIONS = format!("{:?}", options)
        );
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &text);
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> anyhow::Result<()> {
        let text = crate::t_fmt!(
            "webui.select_request",
            PROMPT = prompt,
            ID = request_id,
            OPTIONS = format!("{:?}", options)
        );
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &text);
        Ok(())
    }

    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::runtime::controller::QuestionItem],
    ) -> anyhow::Result<()> {
        let mut text = crate::t_fmt!("webui.questions_title", ID = request_id);
        for q in questions {
            text.push_str(&crate::t_fmt!(
                "webui.question_item",
                HEADER = q.header,
                QUESTION = q.question
            ));
            for opt in &q.options {
                text.push_str(&crate::t_fmt!(
                    "webui.question_option",
                    LABEL = opt.label,
                    DESCRIPTION = opt.description
                ));
            }
        }
        text.push('\n');
        broadcast_event(&self.session_id, "webui", &self.session_id, "system", &text);
        Ok(())
    }
}

/// Spawn a long-running poller task for a WebUI session.
/// The task loops as long as the Claude session remains active,
/// handling multiple user messages without re-spawning.
fn spawn_webui_poller_task(
    channel_id: String,
    session_id: String,
    controller: std::sync::Arc<Mutex<AgentController>>,
) -> tokio::task::AbortHandle {
    let handle = tokio::spawn(async move {
        info!("[WebUI] Poller task started for session {}", session_id);
        loop {
            let poller = {
                let ctrl = controller.lock().await;
                if !ctrl.is_session_active().await {
                    info!(
                        "[WebUI] Session {} no longer active, poller exiting",
                        session_id
                    );
                    break;
                }
                AgentEventPoller::from_controller(&ctrl)
            };

            let sink = WebUIEventSink {
                session_id: session_id.clone(),
            };
            // WebUI: local/SSE, allow higher flush frequency.
            let mut sink = crate::runtime::event_poller::BufferedSink::new(
                sink,
                std::time::Duration::from_millis(WEBUI_FLUSH_INTERVAL_MS),
                WEBUI_MAX_BUFFER_CHARS,
            );
            if let Err(e) = poller.run_buffered(&mut sink).await {
                tracing::warn!("[WebUI] Poller error for session {}: {}", session_id, e);
            }

            // After Done, check if session is still active (next message may arrive)
            let still_active = {
                let ctrl = controller.lock().await;
                ctrl.is_session_active().await
            };
            if !still_active {
                break;
            }
        }
        info!("[WebUI] Poller task for channel {} ended", channel_id);
    });
    handle.abort_handle()
}
