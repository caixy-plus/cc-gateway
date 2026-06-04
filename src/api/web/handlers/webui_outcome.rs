//! Map shared [`ChatCommandOutcome`] to WebUI HTTP + SSE (presentation only).

use axum::http::StatusCode;
use serde_json::json;

use crate::session::channel_command::ChatCommandOutcome;
use crate::session::channel_manager::{ActiveAgentRuntime, GLOBAL_CHANNEL_SESSIONS};
use crate::session::outcome_text;
use crate::web::handlers::session::{ensure_webui_poller_task, json_error, AppState};
use crate::web::state::broadcast_event;

pub(crate) struct WebuiMessageHttpResult {
    pub status: StatusCode,
    pub body: String,
}

fn reply_response(text: &str) -> WebuiMessageHttpResult {
    WebuiMessageHttpResult {
        status: StatusCode::OK,
        body: json!({ "response": text }).to_string(),
    }
}

pub(crate) async fn deliver_chat_outcome(
    state: &AppState,
    channel_id: &str,
    session_id: &str,
    outcome: ChatCommandOutcome,
) -> WebuiMessageHttpResult {
    match outcome {
        ChatCommandOutcome::NoOp => reply_response(""),
        ChatCommandOutcome::Reply(text)
        | ChatCommandOutcome::Error(text)
        | ChatCommandOutcome::ThinkingShown { message: text }
        | ChatCommandOutcome::ThinkingHidden { message: text }
        | ChatCommandOutcome::WorkDirChanged { message: text, .. }
        | ChatCommandOutcome::CurrentDir { message: text, .. }
        | ChatCommandOutcome::DirCreated { message: text, .. } => {
            broadcast_event(session_id, "webui", session_id, "system", &text);
            reply_response(&text)
        }
        ChatCommandOutcome::Stopped { message } => {
            broadcast_event(session_id, "webui", session_id, "system", &message);
            WebuiMessageHttpResult {
                status: StatusCode::OK,
                body: json!({
                    "response": message,
                    "status": "stopped",
                    "session_id": session_id
                })
                .to_string(),
            }
        }
        ChatCommandOutcome::Started { message } => {
            broadcast_event(session_id, "webui", session_id, "system", &message);
            WebuiMessageHttpResult {
                status: StatusCode::OK,
                body: json!({ "response": message, "status": "started" }).to_string(),
            }
        }
        ChatCommandOutcome::ListDir { dir, dirs } => {
            let text = outcome_text::format_list_dir(&dir, &dirs);
            broadcast_event(session_id, "webui", session_id, "system", &text);
            reply_response(&text)
        }
        ChatCommandOutcome::SelectAgent {
            current,
            options: _,
        } => {
            let text = outcome_text::format_select_agent(&state.agent_settings, &current);
            broadcast_event(session_id, "webui", session_id, "system", &text);
            reply_response(&text)
        }
        ChatCommandOutcome::SelectModel {
            provider,
            current,
            options,
        } => {
            let text = outcome_text::format_select_model(&provider, current.as_deref(), &options);
            broadcast_event(session_id, "webui", session_id, "system", &text);
            reply_response(&text)
        }
        ChatCommandOutcome::History { sessions } => {
            let text = outcome_text::format_history(&sessions);
            broadcast_event(session_id, "webui", session_id, "system", &text);
            reply_response(&text)
        }
        ChatCommandOutcome::ForwardToAgent { active, text } => {
            let ctrl = active.controller.lock().await;
            if let Err(e) = ctrl.send_message(&text).await {
                let msg = crate::t_fmt!("forward.failed_send", ERR = e);
                broadcast_event(session_id, "webui", session_id, "system", &msg);
                return WebuiMessageHttpResult {
                    status: StatusCode::INTERNAL_SERVER_ERROR,
                    body: json_error("webui.forward_failed", msg).to_string(),
                };
            }
            drop(ctrl);
            broadcast_event(session_id, "webui", session_id, "user", &text);
            GLOBAL_CHANNEL_SESSIONS.touch_agent_session(session_id);
            ensure_webui_poller_task(channel_id, session_id, active.controller.clone()).await;
            WebuiMessageHttpResult {
                status: StatusCode::OK,
                body: json!({ "status": "forwarded" }).to_string(),
            }
        }
    }
}

/// After [`ChatCommandExecutor`] updates context, sync WebUI runtime maps.
pub(crate) fn sync_webui_active_after_execute(
    channel_id: &str,
    session_id: &str,
    active: Option<ActiveAgentRuntime>,
) {
    match active {
        Some(a) => {
            GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(channel_id, a);
        }
        None => {
            GLOBAL_CHANNEL_SESSIONS.remove_webui_active_agent(channel_id, session_id);
        }
    }
}
