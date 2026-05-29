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

#[derive(Deserialize)]
pub struct ApprovedChatRequest {
    pub platform: String,
    pub chat_id: String,
}

#[derive(Deserialize)]
pub struct SetApprovalEnabledRequest {
    pub platform: String,
    pub chat_id: String,
    pub enabled: bool,
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

pub async fn handle_list_approved() -> Json<serde_json::Value> {
    let list: Vec<serde_json::Value> = GLOBAL_PAIRING_MANAGER
        .list_approved()
        .iter()
        .map(|a| {
            json!({
                "platform": a.platform,
                "chat_id": a.chat_id,
                "approved_at": a.approved_at,
                "enabled": a.enabled,
            })
        })
        .collect();
    Json(json!({ "approved": list }))
}

/// Suspend (取消放行) or resume (重新放行) a kept approval record. The record is
/// preserved either way, so resuming never requires a new pairing handshake.
pub async fn handle_set_approval_enabled(
    Json(req): Json<SetApprovalEnabledRequest>,
) -> (StatusCode, String) {
    if GLOBAL_PAIRING_MANAGER.set_approval_enabled(&req.platform, &req.chat_id, req.enabled) {
        let body = json!({ "status": "ok", "enabled": req.enabled });
        (StatusCode::OK, body.to_string())
    } else {
        let body = json!({ "error": "Approval record not found" });
        (StatusCode::NOT_FOUND, body.to_string())
    }
}

/// Permanently delete an approval record. The chat must pair again to regain
/// access.
pub async fn handle_delete_approval(
    Json(req): Json<ApprovedChatRequest>,
) -> (StatusCode, String) {
    if GLOBAL_PAIRING_MANAGER.delete_approval(&req.platform, &req.chat_id) {
        let body = json!({ "status": "deleted" });
        (StatusCode::OK, body.to_string())
    } else {
        let body = json!({ "error": "Approval record not found" });
        (StatusCode::NOT_FOUND, body.to_string())
    }
}
