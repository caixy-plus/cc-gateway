use crate::config::loader::ConfigLoader;
use crate::session::pairing::GLOBAL_PAIRING_MANAGER;
use crate::web::handlers::session::AppState;
use axum::{extract::State, http::StatusCode, response::Json};
use serde_json::json;

fn mask_secret(s: &str) -> String {
    crate::config::secrets::mask_secret(s)
}

fn is_masked_value(incoming: &str, existing_secret: &str) -> bool {
    crate::config::secrets::is_masked_secret(incoming, existing_secret)
}

pub async fn handle_get_config(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut config = crate::config::loader::ConfigLoader::load().unwrap_or_default();
    crate::config::platform_registry::mask_platform_secrets_in_config(&mut config);
    if let Some(ref token) = config.webui_token {
        config.webui_token = Some(mask_secret(token));
    }
    Json(serde_json::json!({
        "config": config,
        "effective": {
            "show_thinking": state.show_thinking,
            "default_dir": state.default_dir,
            "agent_settings": state.agent_settings,
        },
        "agents": crate::config::agent_registry::build_agents_api_response(&state.agent_settings),
        "restart_policy": crate::config::restart_policy::restart_policy_metadata(),
    }))
}

/// Integrated agent catalog and per-provider profile settings for WebUI / API clients.
pub async fn handle_get_agents(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(crate::config::agent_registry::build_agents_api_response(
        &state.agent_settings,
    ))
}

pub async fn handle_save_config(Json(body): Json<serde_json::Value>) -> (StatusCode, String) {
    let path = match ConfigLoader::config_path() {
        Ok(p) => p,
        Err(e) => {
            let body = json!({ "error": format!("Config path error: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    let before = ConfigLoader::load_from(&path).unwrap_or_default();

    let mut config = before.clone();

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
        match crate::config::agent_registry::agent_profiles_from_api_json(v) {
            Ok(c) => config.agent = c,
            Err(e) => {
                let body = json!({ "error": e.to_string() });
                return (StatusCode::BAD_REQUEST, body.to_string());
            }
        }
    }
    if let Some(platforms) = body.get("platforms").and_then(|v| v.as_object()) {
        crate::config::platform_registry::apply_platforms_from_json(
            &mut config,
            platforms,
            &is_masked_value,
        );
    }
    crate::config::platform_registry::apply_legacy_platform_sections_from_json(
        &mut config,
        &body,
        &is_masked_value,
    );

    let assessment = crate::config::restart_policy::assess_config_changes(&before, &config);

    match ConfigLoader::save(&config) {
        Ok(()) => {
            // Apply the pairing flags live so the toggle takes effect without
            // a daemon restart (running platforms read the manager, not config).
            crate::config::platform_registry::apply_pairing_flags_from_config(&config);
            let body = json!({
                "status": "saved",
                "requires_restart": assessment.requires_restart,
                "restart_fields": assessment.restart_fields,
                "live_fields": assessment.live_fields,
            });
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
    Json(crate::config::platform_registry::build_platforms_api_response(&config))
}

/// Quick toggle for a platform's `require_pairing` flag. Applies live (no
/// restart) and persists to config.json so it survives the next restart.
pub async fn handle_set_require_pairing(
    Json(body): Json<serde_json::Value>,
) -> (StatusCode, String) {
    let platform = body.get("platform").and_then(|v| v.as_str()).unwrap_or("");
    let required = match body.get("require_pairing").and_then(|v| v.as_bool()) {
        Some(v) => v,
        None => {
            let body = json!({ "error": "Missing 'require_pairing' boolean" });
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
    };
    if crate::config::platform_registry::def_by_id(platform).is_none() {
        let body = json!({ "error": "Unknown platform" });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    // Apply live first so it takes effect immediately.
    GLOBAL_PAIRING_MANAGER.set_require_pairing(platform, required);

    // Persist to config.json so the choice survives a restart.
    if let Ok(path) = ConfigLoader::config_path() {
        let mut config = ConfigLoader::load_from(&path).unwrap_or_default();
        if !crate::config::platform_registry::set_require_pairing_in_config(
            &mut config,
            platform,
            required,
        ) {
            let body = json!({ "error": "Unknown platform" });
            return (StatusCode::BAD_REQUEST, body.to_string());
        }
        if let Err(e) = ConfigLoader::save(&config) {
            let body = json!({ "error": format!("Failed to persist config: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    }

    let body = json!({ "status": "ok", "platform": platform, "require_pairing": required });
    (StatusCode::OK, body.to_string())
}
