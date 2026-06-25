use axum::{
    extract::{Json, Multipart, Path, Query, State},
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

use crate::config::model::{AgentProfiles, AgentProvider};
use crate::runtime::controller::AgentController;
use crate::runtime::event_poller::EventPollSink;
use crate::runtime::session_poller::{spawn_session_poller, SessionPollerConfig};
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};
use crate::session::channel_manager::ActiveAgentRuntime;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::chat_flow;
use crate::web::state::{broadcast_event, EVENT_BUS};

async fn resume_webui_agent(
    session_id: &str,
    state: &AppState,
) -> anyhow::Result<ActiveAgentRuntime> {
    GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_for_platform(
            session_id,
            &state.default_dir,
            state.agent_settings.clone(),
            state.show_thinking,
            None,
            Some(crate::web::files::mcp_context_for_session(session_id)),
            None,
        )
        .await
}

#[derive(Clone)]
pub struct AppState {
    pub agent_settings: AgentProfiles,
    pub show_thinking: bool,
    pub default_dir: String,
    pub daemon_config_path: Option<PathBuf>,
    /// CIDR allowlist for IP-based access control.
    pub allowed_ips: Vec<String>,
    /// Token for WebUI access control. None = disabled.
    pub webui_token: Option<String>,
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

pub(crate) fn json_error(key: &str, msg: impl Into<String>) -> serde_json::Value {
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
    /// Filter by platform id: `webui`, `feishu`, `telegram`, or `all`.
    pub(crate) platform: Option<String>,
    pub(crate) source: Option<String>,
    #[serde(alias = "channelId")]
    pub(crate) channel_id: Option<String>,
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

    let platform_filter = query.platform.unwrap_or_else(|| "webui".to_string());
    let source_filter = query.source.clone();
    let channel_filter = query.channel_id.clone();

    let beijing_offset = FixedOffset::east_opt(8 * 3600).unwrap();

    let mapped: Vec<serde_json::Value> = sessions
        .into_iter()
        .filter(|s| {
            let Some(channel) = channels.get(&s.channel_session_id) else {
                return platform_filter == "all";
            };
            if let Some(ref cid) = channel_filter {
                if cid != "all" && s.channel_session_id != *cid {
                    return false;
                }
            } else if platform_filter != "all"
                && !channel.platform.eq_ignore_ascii_case(&platform_filter)
            {
                return false;
            }
            if let Some(ref src) = source_filter {
                if src != "all"
                    && !channel
                        .source
                        .to_string()
                        .eq_ignore_ascii_case(src)
                {
                    return false;
                }
            }
            true
        })
        .map(|s| {
            let channel = channels.get(&s.channel_session_id);
            let created_at_local = s.created_at.with_timezone(&beijing_offset);
            let stopped_at_local = s.stopped_at.map(|t| t.with_timezone(&beijing_offset));
            let updated_at_local = s.updated_at.map(|t| t.with_timezone(&beijing_offset));
            let is_webui = channel
                .map(|c| c.source == crate::session::channel_model::SessionSource::WebUI)
                .unwrap_or(true);
            // Avoid leaking test identifiers (e.g. `test-openid`) in the WebUI list.
            // Non-WebUI sessions are view-only in WebUI; keep their titles generic when they look like test data.
            let display_title = if is_webui {
                s.title.clone()
            } else {
                let platform = channel
                    .map(|c| c.platform.as_str())
                    .unwrap_or("unknown");
                let channel_id = channel.map(|c| c.channel_id.as_str()).unwrap_or("");
                let looks_like_test = s.title.contains("test-openid")
                    || channel_id.contains("test-openid")
                    || s.title.contains("u:test-")
                    || s.title.contains("g:test-");
                if looks_like_test {
                    format!("{} (test)", platform)
                } else {
                    s.title.clone()
                }
            };
            serde_json::json!({
                "id": s.id,
                "title": display_title,
                "source": channel.map(|c| c.source.to_string()).unwrap_or_else(|| "webui".to_string()),
                "platform": channel.map(|c| c.platform.clone()).unwrap_or_else(|| "webui".to_string()),
                // WebUI doesn't need to expose per-chat identifiers for non-WebUI sources.
                "chat_id": if is_webui { s.channel_session_id } else { "".to_string() },
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

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct StartSessionRequest {
    /// Optional provider override for **first-time start** (WebUI empty state).
    /// Omit when **resuming** after stop — uses the session record's stored provider.
    pub(crate) provider: Option<String>,
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
    let work_dir = req
        .work_dir
        .filter(|dir| dir.trim() != "~")
        .unwrap_or_else(|| state.default_dir.clone());
    let expanded = shellexpand::tilde(&work_dir).to_string();
    let title = req
        .title
        .map_or_else(|| derive_title_from_dir(&expanded), |t| t);

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
    Json(req): Json<StartSessionRequest>,
) -> (StatusCode, String) {
    let provider_override = req.provider.as_deref().map(AgentProvider::parse_str);
    let requested_provider = provider_override.clone();
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
        .resume_agent_session_for_platform(
            &session_id,
            &state.default_dir,
            state.agent_settings.clone(),
            state.show_thinking,
            None,
            Some(crate::web::files::mcp_context_for_session(&session_id)),
            provider_override,
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
            let mut body = json!({
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
            let caps = crate::config::agent_registry::capabilities_for(&session.stored_provider());
            if caps.restart_shows_fresh_hint
                && crate::session::channel_manager::gateway_session_has_user_history(&session)
            {
                body["notice"] = json!(crate::t!("builtin.session_restarted_pi_hint"));
            }
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let provider = requested_provider.unwrap_or_else(|| {
                GLOBAL_CHANNEL_SESSIONS
                    .get_agent_session(&session_id)
                    .map(|s| s.stored_provider())
                    .unwrap_or(state.agent_settings.default.clone())
            });
            let body = json_error(
                "webui.resume_failed",
                crate::command::agents::failed_start_agent_message(&provider, e),
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
        None => match resume_webui_agent(&session_id, &state).await {
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
                let provider = GLOBAL_CHANNEL_SESSIONS
                    .get_agent_session(&session_id)
                    .map(|s| s.stored_provider())
                    .unwrap_or(state.agent_settings.default.clone());
                let body = json_error(
                    "webui.resume_failed",
                    crate::command::agents::failed_start_agent_message(&provider, e),
                );
                return (StatusCode::NOT_FOUND, body.to_string());
            }
        },
    };

    // Guard: if the controller's session died since we last checked, try to restart it.
    {
        let ctrl = active.controller.lock().await;
        if !ctrl.is_session_active().await {
            drop(ctrl);
            let _ = GLOBAL_CHANNEL_SESSIONS
                .stop_webui_session(&channel_id, &session_id)
                .await;

            match resume_webui_agent(&session_id, &state).await {
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
                    let provider = GLOBAL_CHANNEL_SESSIONS
                        .get_agent_session(&session_id)
                        .map(|s| s.stored_provider())
                        .unwrap_or(state.agent_settings.default.clone());
                    let body = json_error(
                        "webui.resume_failed",
                        crate::command::agents::failed_start_agent_message(&provider, e),
                    );
                    return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
                }
            }
        }
    }

    ensure_webui_poller_task(&channel_id, &session_id, active.controller.clone()).await;

    let channel_work_dir = GLOBAL_CHANNEL_SESSIONS
        .get_channel(&channel_id)
        .map(|c| c.work_dir)
        .unwrap_or_else(|| active.agent_session.work_dir.clone());

    let mut context = ChatCommandContext::new(
        channel_id.clone(),
        active.agent_session.title.clone(),
        channel_work_dir,
        Some(active.clone()),
    )
    .with_webui_session(session_id.clone());

    let executor = ChatCommandExecutor::new(
        &state.default_dir,
        state.agent_settings.clone(),
        state.show_thinking,
    );

    let outcome =
        match chat_flow::route_and_execute(&active.router, &executor, &mut context, &message).await
        {
            Ok(o) => o,
            Err(e) => {
                let body = json_error("webui.command_failed", e.to_string());
                return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
            }
        };

    super::webui_outcome::sync_webui_active_after_execute(
        &channel_id,
        &session_id,
        context.active_agent.clone(),
    );
    if let Some(ref a) = context.active_agent {
        ensure_webui_poller_task(&channel_id, &session_id, a.controller.clone()).await;
    }

    let http = super::webui_outcome::deliver_chat_outcome(
        &state,
        &channel_id,
        &session_id,
        &message,
        outcome,
    )
    .await;
    (http.status, http.body)
}

pub async fn handle_upload_file(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    mut multipart: Multipart,
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

    let mut caption = String::new();
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_name = String::from("file");
    let mut content_type: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let Some(name) = field.name() else {
            continue;
        };
        match name {
            "message" | "caption" => {
                if let Ok(text) = field.text().await {
                    caption = text;
                }
            }
            "file" => {
                file_name = field.file_name().unwrap_or("file").to_string();
                content_type = field.content_type().map(str::to_string);
                match field.bytes().await {
                    Ok(bytes) => file_bytes = Some(bytes.to_vec()),
                    Err(e) => {
                        let body = json_error(
                            "webui.upload_read_failed",
                            format!("Failed to read upload: {e}"),
                        );
                        return (StatusCode::BAD_REQUEST, body.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    let Some(bytes) = file_bytes else {
        let body = json_error(
            "webui.upload_missing_file",
            crate::t!("webui.upload_missing_file"),
        );
        return (StatusCode::BAD_REQUEST, body.to_string());
    };

    let saved =
        match crate::web::files::save_upload_for_webui(&bytes, &file_name, content_type.as_deref())
            .await
        {
            Ok(s) => s,
            Err(e) => {
                let body = json_error("webui.upload_failed", e.to_string());
                return (StatusCode::BAD_REQUEST, body.to_string());
            }
        };

    let media_key = match crate::web::files::media_storage_basename(&saved.path) {
        Ok(k) => k,
        Err(e) => {
            let body = json_error("webui.upload_failed", e.to_string());
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };
    let display_name = sanitize_display_name(&file_name);
    let size = bytes.len() as u64;

    let local_path = saved.path.display().to_string();
    crate::web::files::broadcast_file_attachment(
        &session_id,
        "user",
        &media_key,
        &display_name,
        size,
        saved.is_image,
        &local_path,
    );

    let agent_text = crate::web::files::format_webui_upload_agent_message(
        &caption,
        &display_name,
        &saved,
        &bytes,
    );

    let mut forwarded = false;
    if !agent_text.trim().is_empty() {
        if let Some(active) =
            GLOBAL_CHANNEL_SESSIONS.get_webui_active_agent(&channel_id, &session_id)
        {
            if super::webui_outcome::forward_text_to_agent(
                &channel_id,
                &session_id,
                &agent_text,
                &active,
                false,
            )
            .await
            .is_ok()
            {
                forwarded = true;
            }
        }
    }

    let body = json!({
        "status": "ok",
        "media": media_key,
        "name": display_name,
        "size": size,
        "forwarded": forwarded,
    });
    (StatusCode::OK, body.to_string())
}

fn sanitize_display_name(name: &str) -> String {
    std::path::Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string()
}

fn derive_title_from_dir(dir: &str) -> String {
    std::path::Path::new(dir)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("WebUI Session")
        .to_string()
}

pub(crate) async fn ensure_webui_poller_task(
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

    // Always use the cc-gateway agent session id for the history file name
    // (consistent with history/recorder.rs).
    let file_id = &session_id;

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
            history = crate::history::recorder::coalesce_streaming_history(history);
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

    // Use the cc-gateway agent session id (consistent with the recorder).
    let file_id = session_id.clone();

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

#[derive(Deserialize)]
pub struct PermissionRequest {
    pub request_id: String,
    /// "allow" or "deny"
    pub action: String,
    #[serde(default)]
    pub reason: Option<String>,
}

pub async fn handle_permission(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<PermissionRequest>,
) -> (StatusCode, String) {
    use crate::command::CommandAction;

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

    let active = match GLOBAL_CHANNEL_SESSIONS.get_webui_active_agent(&channel_id, &session_id) {
        Some(a) => a,
        None => {
            let body = json_error(
                "webui.session_not_found",
                crate::t!("webui.session_not_found"),
            );
            return (StatusCode::NOT_FOUND, body.to_string());
        }
    };

    let action = match req.action.as_str() {
        "allow" => CommandAction::PermissionAllow {
            request_id: Some(req.request_id.clone()),
        },
        "deny" => CommandAction::PermissionDeny {
            request_id: Some(req.request_id.clone()),
            reason: req.reason.clone(),
        },
        _ => {
            let body = json_error("webui.invalid_action", "Action must be 'allow' or 'deny'");
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
    };

    let channel_work_dir = GLOBAL_CHANNEL_SESSIONS
        .get_channel(&channel_id)
        .map(|c| c.work_dir)
        .unwrap_or_else(|| active.agent_session.work_dir.clone());

    let mut context = ChatCommandContext::new(
        channel_id.clone(),
        active.agent_session.title.clone(),
        channel_work_dir,
        Some(active),
    )
    .with_webui_session(session_id.clone());

    let executor = ChatCommandExecutor::new(
        &state.default_dir,
        state.agent_settings.clone(),
        state.show_thinking,
    );

    match executor.execute(&mut context, action).await {
        Ok(ChatCommandOutcome::Reply(_) | ChatCommandOutcome::NoOp) => {
            let body =
                json!({ "status": "ok", "request_id": req.request_id, "action": req.action });
            (StatusCode::OK, body.to_string())
        }
        Ok(ChatCommandOutcome::Error(e)) => {
            let body = json_error("webui.failed_permission", e);
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
        Ok(_) => {
            let body =
                json!({ "status": "ok", "request_id": req.request_id, "action": req.action });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json_error("webui.failed_permission", format!("Failed: {}", e));
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
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
        let mut body = crate::t_fmt!(
            "webui.permission_request",
            NAME = tool_name,
            ID = request_id
        );
        if let Some(input) = input {
            let pretty = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
            body.push_str("\n\n");
            body.push_str(crate::t!("webui.permission_request_input"));
            body.push('\n');
            body.push_str("```json\n");
            body.push_str(&pretty);
            body.push_str("\n```");
        }
        // Structured format: first line = request_id, rest = display text.
        let content = format!("{}\n{}", request_id, body);
        broadcast_event(
            &self.session_id,
            "webui",
            &self.session_id,
            "permission_request",
            &content,
        );
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
        let content = format!("{}\n{}", request_id, text);
        broadcast_event(
            &self.session_id,
            "webui",
            &self.session_id,
            "permission_request",
            &content,
        );
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
        let content = format!("{}\n{}", request_id, text);
        broadcast_event(
            &self.session_id,
            "webui",
            &self.session_id,
            "permission_request",
            &content,
        );
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
        let content = format!("{}\n{}", request_id, text);
        broadcast_event(
            &self.session_id,
            "webui",
            &self.session_id,
            "permission_request",
            &content,
        );
        Ok(())
    }
}

/// Spawn a long-running poller task for a WebUI session.
fn spawn_webui_poller_task(
    _channel_id: String,
    session_id: String,
    controller: std::sync::Arc<Mutex<AgentController>>,
) -> tokio::task::AbortHandle {
    spawn_session_poller(
        controller,
        SessionPollerConfig {
            log_label: "WebUI",
            session_id: session_id.clone(),
            flush_interval: std::time::Duration::from_millis(WEBUI_FLUSH_INTERVAL_MS),
            max_buffer_chars: WEBUI_MAX_BUFFER_CHARS,
        },
        move || WebUIEventSink {
            session_id: session_id.clone(),
        },
        None,
    )
}
