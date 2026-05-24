use axum::{extract::Json, http::StatusCode};
use serde::Deserialize;
use serde_json::json;

use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

#[derive(Deserialize)]
pub struct CdRequest {
    session_id: Option<String>,
    path: String,
}

#[derive(Deserialize)]
pub struct LlRequest {
    #[allow(dead_code)]
    session_id: Option<String>,
    path: Option<String>,
    show_hidden: Option<bool>,
}

#[derive(Deserialize)]
pub struct SessionCmdRequest {
    session_id: Option<String>,
}

fn webui_channel_id() -> Option<String> {
    GLOBAL_CHANNEL_SESSIONS
        .list_channels()
        .into_iter()
        .find(|c| c.platform == "webui")
        .map(|c| c.id)
}

async fn get_work_dir_async(_session_id: Option<&str>) -> String {
    if let Some(channel_id) = webui_channel_id() {
        if let Some(runtime) = GLOBAL_CHANNEL_SESSIONS.get_webui_runtime(&channel_id) {
            if let Some(ref active) = runtime.active_claude {
                let ctrl = active.controller.lock().await;
                let wd = ctrl.get_work_dir().await;
                if !wd.is_empty() {
                    return wd;
                }
            }
            return runtime.channel_session.work_dir.clone();
        }
    }
    std::env::current_dir()
        .map(|d| d.to_string_lossy().to_string())
        .unwrap_or_else(|_| "~".to_string())
}

async fn set_session_work_dir(_session_id: Option<&str>, dir: String) {
    if let Some(channel_id) = webui_channel_id() {
        if let Some(runtime) = GLOBAL_CHANNEL_SESSIONS.get_webui_runtime(&channel_id) {
            if let Some(ref active) = runtime.active_claude {
                let ctrl = active.controller.lock().await;
                ctrl.init_work_dir(dir.clone()).await;
            }
            GLOBAL_CHANNEL_SESSIONS.update_channel_work_dir(&channel_id, &dir);
            return;
        }
    }
    let _ = std::env::set_current_dir(&dir);
}

pub async fn handle_ll(Json(req): Json<LlRequest>) -> (StatusCode, String) {
    let path = req.path.unwrap_or_else(|| "~".to_string());
    let show_hidden = req.show_hidden.unwrap_or(false);
    let expanded = shellexpand::tilde(&path).to_string();
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
            let body = json!({ "error": format!("Failed to read directory: {}", e) });
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
    let path = shellexpand::tilde(&req.path).to_string();
    let target = std::path::Path::new(&path).canonicalize().unwrap_or_else(|_| std::path::PathBuf::from(&path));
    if !target.is_dir() {
        let body = json!({ "error": format!("Not a directory: {}", path) });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }
    let target_str = target.to_string_lossy().to_string();
    set_session_work_dir(req.session_id.as_deref(), target_str.clone()).await;
    let body = json!({ "dir": target_str });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_cd_default(Json(req): Json<SessionCmdRequest>) -> (StatusCode, String) {
    let default_dir = shellexpand::tilde("~").to_string();
    set_session_work_dir(req.session_id.as_deref(), default_dir.clone()).await;
    let body = json!({ "dir": default_dir });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_help() -> (StatusCode, String) {
    let commands = json!([
        { "cmd": "/help", "desc": "Show available commands" },
        { "cmd": "/quit", "desc": "Quit current Claude session" },
        { "cmd": "/cd <path>", "desc": "Change working directory" },
        { "cmd": "/cd_default", "desc": "Reset to default directory" },
        { "cmd": "/claude [args...]", "desc": "Start a new Claude session" },
        { "cmd": "/pwd", "desc": "Show current working directory" },
        { "cmd": "/ll", "desc": "List directory contents" },
    ]);
    (StatusCode::OK, commands.to_string())
}
