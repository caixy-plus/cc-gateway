use chrono::Utc;
use once_cell::sync::Lazy;
use serde::Serialize;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub session_id: String,
    pub platform: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    pub timestamp: String,
}

pub static EVENT_BUS: Lazy<broadcast::Sender<Event>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(256);
    tx
});

pub fn broadcast_event(session_id: &str, platform: &str, chat_id: &str, role: &str, content: &str) {
    let _ = EVENT_BUS.send(Event {
        session_id: session_id.to_string(),
        platform: platform.to_string(),
        chat_id: chat_id.to_string(),
        role: role.to_string(),
        content: content.to_string(),
        timestamp: Utc::now().to_rfc3339(),
    });
}

// ---------------------------------------------------------------------------
// Deliver queue – allows Claude subprocesses to request file/folder delivery
// back to their bound chat sessions.
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DeliverRequest {
    pub session_id: String,
    pub path: String,
    pub message: Option<String>,
}

pub static DELIVER_BUS: Lazy<broadcast::Sender<DeliverRequest>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(64);
    tx
});

pub fn broadcast_deliver(session_id: &str, path: &str, message: Option<&str>) {
    let _ = DELIVER_BUS.send(DeliverRequest {
        session_id: session_id.to_string(),
        path: path.to_string(),
        message: message.map(|s| s.to_string()),
    });
}
