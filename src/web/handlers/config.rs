use crate::config::loader::ConfigLoader;
use crate::web::handlers::session::AppState;
use axum::{extract::State, http::StatusCode, response::Json};
use serde_json::json;

pub(crate) fn mask_secret(s: &str) -> String {
    if s.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}***{}", &s[..4], &s[s.len() - 4..])
    }
}

/// Check if the frontend sent back a masked secret rather than a real one.
///
/// We consider it masked if it exactly matches the mask we would generate from the
/// existing secret. This avoids false-positives when a real secret happens to contain `***`.
fn is_masked_value(incoming: &str, existing_secret: &str) -> bool {
    if incoming.is_empty() {
        return false;
    }
    incoming == mask_secret(existing_secret)
}

pub async fn handle_get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut config = crate::config::loader::ConfigLoader::load().unwrap_or_default();
    config.feishu.app_secret = mask_secret(&config.feishu.app_secret);
    config.telegram.bot_token = mask_secret(&config.telegram.bot_token);
    Json(serde_json::json!({
        "config": config,
        "effective": {
            "show_thinking": state.show_thinking,
            "default_dir": state.default_dir,
            "agent_settings": state.agent_settings,
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

    let existing = ConfigLoader::load_from(&path).unwrap_or_default();

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
    if let Some(v) = body
        .get("session_retention_per_channel")
        .and_then(|v| v.as_u64())
    {
        config.session_retention_per_channel = v;
    }
    if let Some(v) = body.get("port").and_then(|v| v.as_u64()) {
        if v == 0 || v > 65535 {
            let body = json!({ "error": format!("Port must be between 1 and 65535, got {}", v) });
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
        config.port = v as u16;
    }
    if let Some(v) = body.get("agent") {
        if let Ok(c) = serde_json::from_value(v.clone()) {
            config.agent = c;
        }
    }
    if let Some(v) = body.get("feishu") {
        if let Ok(c) = serde_json::from_value::<crate::config::model::FeishuConfig>(v.clone()) {
            // Preserve real secrets if the frontend sent back masked values
            if !is_masked_value(&c.app_secret, &config.feishu.app_secret) {
                config.feishu.app_secret = c.app_secret;
            }
            config.feishu.enabled = c.enabled;
            config.feishu.app_id = c.app_id;
            config.feishu.allow_from = c.allow_from;
            config.feishu.encrypt_key = c.encrypt_key;
            config.feishu.mode = c.mode;
            config.feishu.webhook_bind = c.webhook_bind;
            config.feishu.require_pairing = c.require_pairing;
        }
    }
    if let Some(v) = body.get("telegram") {
        if let Ok(c) = serde_json::from_value::<crate::config::model::TelegramConfig>(v.clone()) {
            // Preserve real secrets if the frontend sent back masked values
            if !is_masked_value(&c.bot_token, &config.telegram.bot_token) {
                config.telegram.bot_token = c.bot_token;
            }
            config.telegram.enabled = c.enabled;
            config.telegram.allow_from = c.allow_from;
            config.telegram.webhook_url = c.webhook_url;
            config.telegram.require_pairing = c.require_pairing;
        }
    }

    match ConfigLoader::save(&config) {
        Ok(()) => {
            let body = json!({ "status": "saved" });
            (StatusCode::OK, body.to_string())
        }
        Err(e) => {
            let body = json!({ "error": format!("Failed to save config: {}", e) });
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
