use axum::{extract::Multipart, http::StatusCode};
use serde_json::json;
use tracing::info;

use crate::web::state::broadcast_deliver;

pub async fn handle_deliver(mut multipart: Multipart) -> (StatusCode, String) {
    let mut session_id = String::new();
    let mut path = String::new();
    let mut message = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        if let Some(name) = field.name() {
            match name {
                "session_id" => {
                    if let Ok(text) = field.text().await {
                        session_id = text;
                    }
                }
                "path" => {
                    if let Ok(text) = field.text().await {
                        path = text;
                    }
                }
                "message" => {
                    if let Ok(text) = field.text().await {
                        message = text;
                    }
                }
                _ => {}
            }
        }
    }

    if session_id.is_empty() {
        let body = json!({ "error": "Missing 'session_id' field" });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    if path.is_empty() {
        let body = json!({ "error": "Missing 'path' field" });
        return (StatusCode::BAD_REQUEST, body.to_string());
    }

    let expanded = shellexpand::tilde(&path).to_string();
    let msg_opt = if message.is_empty() {
        None
    } else {
        Some(message.as_str())
    };
    info!(
        "Deliver request: session_id={}, path={}, message={:?}",
        session_id, expanded, msg_opt
    );

    broadcast_deliver(&session_id, &expanded, msg_opt);

    let body = json!({
        "message": "Deliver request queued",
        "session_id": session_id,
        "path": expanded
    });
    (StatusCode::OK, body.to_string())
}
