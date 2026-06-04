use axum::{extract::Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

#[derive(Deserialize)]
pub struct CdRequest {
    pub(crate) session_id: Option<String>,
    pub(crate) path: String,
}

#[derive(Deserialize)]
pub struct LlRequest {
    #[allow(dead_code)]
    pub(crate) session_id: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) show_hidden: Option<bool>,
}

#[derive(Deserialize)]
pub struct SessionCmdRequest {
    pub(crate) session_id: Option<String>,
}

fn webui_channel_id() -> Option<String> {
    GLOBAL_CHANNEL_SESSIONS
        .list_channels()
        .into_iter()
        .find(|c| c.platform == "webui")
        .map(|c| c.id)
}

async fn get_work_dir_async(session_id: Option<&str>) -> String {
    if let Some(channel_id) = webui_channel_id() {
        if let Some(runtime) = GLOBAL_CHANNEL_SESSIONS.get_webui_runtime(&channel_id) {
            if let Some(session_id) = session_id {
                if let Some(active) =
                    GLOBAL_CHANNEL_SESSIONS.get_webui_active_agent(&channel_id, session_id)
                {
                    let ctrl = active.controller.lock().await;
                    let wd = ctrl.get_work_dir().await;
                    if !wd.is_empty() {
                        return wd;
                    }
                }
            }
            if let Some(session_id) = session_id {
                if let Some(session) = GLOBAL_CHANNEL_SESSIONS.get_agent_session(session_id) {
                    return session.work_dir;
                }
            }
            return runtime.channel_session.work_dir.clone();
        }
    }
    if let Some(session_id) = session_id {
        if let Some(session) = GLOBAL_CHANNEL_SESSIONS.get_agent_session(session_id) {
            return session.work_dir;
        }
    }
    std::env::current_dir()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~".to_string())
}

async fn set_session_work_dir(session_id: Option<&str>, dir: String) {
    if let Some(session_id) = session_id {
        GLOBAL_CHANNEL_SESSIONS.update_agent_session_work_dir(session_id, &dir);
    }
    if let Some(channel_id) = webui_channel_id() {
        if let Some(_runtime) = GLOBAL_CHANNEL_SESSIONS.get_webui_runtime(&channel_id) {
            if let Some(session_id) = session_id {
                if let Some(active) =
                    GLOBAL_CHANNEL_SESSIONS.get_webui_active_agent(&channel_id, session_id)
                {
                    let ctrl = active.controller.lock().await;
                    ctrl.init_work_dir(dir.clone()).await;
                }
            }
            let _ = GLOBAL_CHANNEL_SESSIONS
                .switch_work_dir(&channel_id, std::path::PathBuf::from(&dir))
                .await;
            return;
        }
    }
    let _ = std::env::set_current_dir(&dir);
}

async fn resolve_requested_dir(
    session_id: Option<&str>,
    requested: &str,
) -> anyhow::Result<String> {
    let current_dir = get_work_dir_async(session_id).await;
    crate::command::workdir::resolve_work_dir_target(
        &current_dir,
        "~",
        std::path::Path::new(requested),
    )
}

pub async fn handle_ll(Json(req): Json<LlRequest>) -> (StatusCode, String) {
    let path = req.path.unwrap_or_else(|| ".".to_string());
    let show_hidden = req.show_hidden.unwrap_or(false);
    let expanded = match resolve_requested_dir(req.session_id.as_deref(), &path).await {
        Ok(dir) => dir,
        Err(e) => {
            let body = json!({ "error_key": "webui.failed_set_dir", "error": e.to_string() });
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
    };
    match std::fs::read_dir(&expanded) {
        Ok(entries) => {
            let mut items: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') && !show_hidden {
                        return false;
                    }
                    e.file_type().ok().map(|t| t.is_dir()).unwrap_or(false)
                })
                .map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    format!("{}/", name)
                })
                .collect();
            items.sort();
            let body = json!({
                "dir": expanded,
                "items": items
            });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error_key": "webui.failed_list_dir", "error": format!("Failed to read directory '{}': {}", expanded, e) });
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_pwd(Json(req): Json<SessionCmdRequest>) -> (StatusCode, String) {
    let dir = get_work_dir_async(req.session_id.as_deref()).await;
    let body = json!({ "dir": dir });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_cd(Json(req): Json<CdRequest>) -> (StatusCode, String) {
    let target_str = match resolve_requested_dir(req.session_id.as_deref(), &req.path).await {
        Ok(dir) => dir,
        Err(e) => {
            let body = json!({ "error_key": "webui.failed_set_dir", "error": e.to_string() });
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
    };
    set_session_work_dir(req.session_id.as_deref(), target_str.clone()).await;
    let body = json!({ "dir": target_str });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_cd_default(
    State(state): State<super::session::AppState>,
    Json(req): Json<SessionCmdRequest>,
) -> (StatusCode, String) {
    let default_dir = shellexpand::tilde(&state.default_dir).to_string();
    set_session_work_dir(req.session_id.as_deref(), default_dir.clone()).await;
    let body = json!({ "dir": default_dir });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_help() -> (StatusCode, String) {
    let commands = json!([
        { "cmd": "/help", "desc": "Show available commands" },
        { "cmd": "/quit", "desc": "Quit current agent session" },
        { "cmd": "/esc [msg]", "desc": "Flush queued messages (Claude: best-effort)" },
        { "cmd": "/stop", "desc": "Stop current generation (Claude: best-effort)" },
        { "cmd": "/clear", "desc": "Clear context" },
        { "cmd": "/status", "desc": "Show agent status (ready / busy)" },
        { "cmd": "/cd <path>", "desc": "Change working directory" },
        { "cmd": "/cd_default", "desc": "Reset to default directory" },
        { "cmd": "/agent [args...]", "desc": "Start a new agent session" },
        { "cmd": "/agents", "desc": "Set this channel default agent" },
        { "cmd": "/pwd", "desc": "Show current working directory" },
        { "cmd": "/ll", "desc": "List directory contents" },
    ]);
    (StatusCode::OK, commands.to_string())
}
