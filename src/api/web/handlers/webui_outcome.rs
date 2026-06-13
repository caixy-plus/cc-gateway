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

/// Echo slash input and gateway replies as chat bubbles (`user` / `assistant`), not centered `system` hints.
fn broadcast_slash_user(session_id: &str, user_message: &str) {
    if user_message.starts_with('/') {
        broadcast_event(session_id, "webui", session_id, "user", user_message);
    }
}

fn broadcast_assistant_reply(session_id: &str, text: &str) {
    if !text.is_empty() {
        broadcast_event(session_id, "webui", session_id, "assistant", text);
    }
}

fn broadcast_command_reply(session_id: &str, user_message: &str, text: &str) {
    broadcast_slash_user(session_id, user_message);
    broadcast_assistant_reply(session_id, text);
}

pub(crate) async fn deliver_chat_outcome(
    state: &AppState,
    channel_id: &str,
    session_id: &str,
    user_message: &str,
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
            broadcast_command_reply(session_id, user_message, &text);
            reply_response(&text)
        }
        ChatCommandOutcome::Stopped { message } => {
            broadcast_command_reply(session_id, user_message, &message);
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
            broadcast_command_reply(session_id, user_message, &message);
            WebuiMessageHttpResult {
                status: StatusCode::OK,
                body: json!({ "response": message, "status": "started" }).to_string(),
            }
        }
        ChatCommandOutcome::ListDir { dir, dirs } => {
            let text = outcome_text::format_list_dir(&dir, &dirs);
            broadcast_command_reply(session_id, user_message, &text);
            reply_response(&text)
        }
        ChatCommandOutcome::SelectAgent {
            current,
            options: _,
        } => {
            let text = outcome_text::format_select_agent(&state.agent_settings, &current);
            broadcast_command_reply(session_id, user_message, &text);
            reply_response(&text)
        }
        ChatCommandOutcome::SelectModel {
            provider,
            current,
            options,
        } => {
            let content = crate::web::interactive::build_model_picker_content(
                &provider,
                current.as_deref(),
                &options,
            );
            broadcast_slash_user(session_id, user_message);
            broadcast_assistant_reply(session_id, &content);
            let hint =
                crate::web::interactive::model_picker_title_line(&provider, current.as_deref());
            reply_response(&hint)
        }
        ChatCommandOutcome::History { sessions } => {
            let text = outcome_text::format_history(&sessions);
            broadcast_command_reply(session_id, user_message, &text);
            reply_response(&text)
        }
        ChatCommandOutcome::ForwardToAgent { active, text } => {
            match forward_text_to_agent(channel_id, session_id, &text, &active, true).await {
                Ok(()) => WebuiMessageHttpResult {
                    status: StatusCode::OK,
                    body: json!({ "status": "forwarded" }).to_string(),
                },
                Err(e) => {
                    let msg = crate::t_fmt!("forward.failed_send", ERR = e);
                    broadcast_command_reply(session_id, user_message, &msg);
                    WebuiMessageHttpResult {
                        status: StatusCode::INTERNAL_SERVER_ERROR,
                        body: json_error("webui.forward_failed", msg).to_string(),
                    }
                }
            }
        }
    }
}

/// Push `text` to the active agent. When `echo_user_to_ui` is false, skip a duplicate user
/// bubble (e.g. WebUI file upload already rendered an attachment card).
pub(crate) async fn forward_text_to_agent(
    channel_id: &str,
    session_id: &str,
    text: &str,
    active: &ActiveAgentRuntime,
    echo_user_to_ui: bool,
) -> anyhow::Result<()> {
    let ctrl = active.controller.lock().await;
    ctrl.send_message(text)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    drop(ctrl);
    if echo_user_to_ui {
        broadcast_event(session_id, "webui", session_id, "user", text);
    }
    GLOBAL_CHANNEL_SESSIONS.touch_agent_session(session_id);
    ensure_webui_poller_task(channel_id, session_id, active.controller.clone()).await;
    Ok(())
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
