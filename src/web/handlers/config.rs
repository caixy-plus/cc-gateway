use crate::config::loader::ConfigLoader;
use crate::session::pairing::GLOBAL_PAIRING_MANAGER;
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
    if let Some(ref token) = config.webui_token {
        config.webui_token = Some(mask_secret(token));
    }
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
    if let Some(v) = body.get("bind_address").and_then(|v| v.as_str()) {
        if !v.is_empty() {
            config.bind_address = v.to_string();
        }
    }
    if let Some(v) = body.get("allowed_ips") {
        if let Some(arr) = v.as_array() {
            config.allowed_ips = arr
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
        }
    }
    // webui_token: null/absent = clear; string = set.
    // Preserve existing token if the frontend sent back the masked value.
    if body.get("webui_token").is_some() {
        let incoming = body
            .get("webui_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if incoming.is_empty() {
            config.webui_token = None;
        } else if let Some(ref existing) = config.webui_token {
            if !is_masked_value(&incoming, existing) {
                config.webui_token = Some(incoming);
            }
        } else {
            config.webui_token = Some(incoming);
        }
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
            config.telegram.require_pairing = c.require_pairing;
        }
    }

    match ConfigLoader::save(&config) {
        Ok(()) => {
            // Apply the pairing flags live so the toggle takes effect without
            // a daemon restart (running platforms read the manager, not config).
            crate::session::pairing::GLOBAL_PAIRING_MANAGER
                .set_require_pairing("feishu", config.feishu.require_pairing);
            crate::session::pairing::GLOBAL_PAIRING_MANAGER
                .set_require_pairing("telegram", config.telegram.require_pairing);
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
            "state": crate::platform::status::get_state("feishu").as_str(),
            "require_pairing": GLOBAL_PAIRING_MANAGER.require_pairing("feishu"),
        }));
    }
    if config.telegram.enabled {
        platforms.push(serde_json::json!({
            "name": "telegram",
            "enabled": true,
            "state": crate::platform::status::get_state("telegram").as_str(),
            "require_pairing": GLOBAL_PAIRING_MANAGER.require_pairing("telegram"),
        }));
    }

    Json(serde_json::json!({ "platforms": platforms }))
}

/// Quick toggle for a platform's `require_pairing` flag. Applies live (no
/// restart) and persists to config.json so it survives the next restart.
pub async fn handle_set_require_pairing(Json(body): Json<serde_json::Value>) -> (StatusCode, String) {
    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    let required = match body.get("require_pairing").and_then(|v| v.as_bool()) {
        Some(v) => v,
        None => {
            let body = json!({ "error": "Missing 'require_pairing' boolean" });
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
    };
    if platform != "feishu" && platform != "telegram" {
        let body = json!({ "error": "Unknown platform" });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    // Apply live first so it takes effect immediately.
    GLOBAL_PAIRING_MANAGER.set_require_pairing(platform, required);

    // Persist to config.json so the choice survives a restart.
    if let Ok(path) = ConfigLoader::config_path() {
        let mut config = ConfigLoader::load_from(&path).unwrap_or_default();
        match platform {
            "feishu" => config.feishu.require_pairing = required,
            "telegram" => config.telegram.require_pairing = required,
            _ => {}
        }
        if let Err(e) = ConfigLoader::save(&config) {
            let body = json!({ "error": format!("Failed to persist config: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    }

    let body = json!({ "status": "ok", "platform": platform, "require_pairing": required });
    (StatusCode::OK, body.to_string())
}
