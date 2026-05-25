use axum::{extract::State, http::StatusCode, response::Json};
use serde_json::json;
use std::fs;

use crate::config::loader::ConfigLoader;
use crate::web::handlers::session::AppState;

pub(crate) fn mask_secret(s: &str) -> String {
    if s.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}***{}", &s[..4], &s[s.len() - 4..])
    }
}

pub async fn handle_get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut config = crate::config::loader::ConfigLoader::load().unwrap_or_default();
    // Mask secrets before returning to client
    config.feishu.app_secret = mask_secret(&config.feishu.app_secret);
    config.telegram.bot_token = mask_secret(&config.telegram.bot_token);
    config.feishu.app_id = mask_secret(&config.feishu.app_id);
    Json(serde_json::json!({
        "config": config,
        "effective": {
            "show_thinking": state.show_thinking,
            "default_dir": state.default_dir,
            "agent_config": state.claude_config,
            "claude_config": state.claude_config,
        }
    }))
}

pub async fn handle_save_config(Json(body): Json<serde_json::Value>) -> (StatusCode, String) {
    let path = match ConfigLoader::config_path() {
        Ok(p) => p,
        Err(e) => {
            let body = json!({ "error": format!("Config path error: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    // Merge with existing config to preserve fields not sent by client
    let existing = match ConfigLoader::load_from(&path) {
        Ok(c) => c,
        Err(_) => crate::config::model::GatewayConfig::default(),
    };

    let mut config = existing;

    if let Some(v) = body.get("show_thinking").and_then(|v| v.as_bool()) {
        config.show_thinking = v;
    }
    if let Some(v) = body.get("default_dir").and_then(|v| v.as_str()) {
        config.default_dir = v.to_string();
    }
    if let Some(v) = body.get("media_retention_days").and_then(|v| v.as_u64()) {
        config.media_retention_days = v;
    }
    if let Some(v) = body.get("port").and_then(|v| v.as_u64()) {
        config.port = v as u16;
    }
    if let Some(v) = body.get("claude") {
        if let Ok(c) = serde_json::from_value(v.clone()) {
            config.claude = c;
        }
    }
    if let Some(v) = body.get("agent") {
        if let Ok(c) = serde_json::from_value(v.clone()) {
            config.agent = Some(c);
        }
    }
    if let Some(v) = body.get("feishu") {
        if let Ok(c) = serde_json::from_value(v.clone()) {
            config.feishu = c;
        }
    }
    if let Some(v) = body.get("telegram") {
        if let Ok(c) = serde_json::from_value(v.clone()) {
            config.telegram = c;
        }
    }

    match serde_json::to_string_pretty(&config) {
        Ok(content) => {
            if let Err(e) = fs::write(&path, content) {
                let body = json!({ "error": format!("Failed to write config: {}", e) });
                return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
            }
            let body = json!({ "status": "saved" });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to serialize config: {}", e) });
            (StatusCode::INTERNAL_SERVER_ERROR, body.to_string())
        }
    }
}

pub async fn handle_get_platforms() -> Json<serde_json::Value> {
    let config = crate::config::loader::ConfigLoader::load().unwrap_or_default();
    let mut platforms = Vec::new();

    if config.feishu.enabled {
        platforms.push(serde_json::json!({
            "name": "feishu",
            "enabled": true,
            "mode": config.feishu.mode,
            "allow_from": config.feishu.allow_from,
        }));
    }
    if config.telegram.enabled {
        platforms.push(serde_json::json!({
            "name": "telegram",
            "enabled": true,
            "allow_from": config.telegram.allow_from,
        }));
    }

    Json(serde_json::json!({ "platforms": platforms }))
}
