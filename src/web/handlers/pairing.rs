use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use serde_json::json;

use crate::session::pairing::GLOBAL_PAIRING_MANAGER;
use crate::web::handlers::session::AppState;

use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

#[derive(Deserialize)]
pub struct PairingCodeRequest {
    pub pairing_code: String,
}

pub async fn handle_list_pending() -> Json<serde_json::Value> {
    let list: Vec<serde_json::Value> = GLOBAL_PAIRING_MANAGER
        .list_pending()
        .iter()
        .map(|p| {
            json!({
                "pairing_code": p.pairing_code,
                "platform": p.platform,
                "chat_id": p.chat_id,
                "created_at": p.created_at.to_rfc3339(),
            })
        })
        .collect();
    Json(json!({ "pending": list }))
}

pub async fn handle_approve(
    State(state): State<AppState>,
    Json(req): Json<PairingCodeRequest>,
) -> (StatusCode, String) {
    let Some((platform, chat_id)) = GLOBAL_PAIRING_MANAGER.approve(&req.pairing_code) else {
        let body = json!({ "error": "Pairing code not found or already processed" });
        return (StatusCode::NOT_FOUND, body.to_string());
    };

    // Create the channel session to mark this chat as approved.
    GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel(&platform, &chat_id, &state.default_dir)
        .await;

    let body = json!({
        "status": "approved",
        "platform": platform,
        "chat_id": chat_id,
    });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_reject(
    Json(req): Json<PairingCodeRequest>,
) -> (StatusCode, String) {
    if GLOBAL_PAIRING_MANAGER.reject(&req.pairing_code) {
        let body = json!({ "status": "rejected" });
        (StatusCode::OK, body.to_string())
    } else {
        let body = json!({ "error": "Pairing code not found or already processed" });
        (StatusCode::NOT_FOUND, body.to_string())
    }
}
