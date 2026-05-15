use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::claude::controller::ClaudeController;

pub struct ForwardCommand {
    controller: Arc<Mutex<ClaudeController>>,
}

impl ForwardCommand {
    pub fn new(controller: Arc<Mutex<ClaudeController>>) -> Self {
        Self { controller }
    }

    /// Handle /cc/ prefix commands - forward to Claude as native slash commands
    pub async fn handle(&self, message: &str) -> Option<String> {
        // Strip /cc/ prefix
        let cmd = message.strip_prefix("/cc/").unwrap_or(message);
        let claude_cmd = format!("/{}", cmd);

        debug!("Forwarding slash command to Claude: {}", claude_cmd);

        let ctrl = self.controller.lock().await;
        if !ctrl.is_session_active().await {
            return Some("No active Claude session. Use /claude to start one.".to_string());
        }

        match ctrl.send_message(&claude_cmd).await {
            Ok(()) => None, // Response will come through event channel
            Err(e) => Some(format!("Failed to send command: {}", e)),
        }
    }

    /// Handle regular messages - forward to Claude
    pub async fn handle_regular(&self, message: &str) -> Option<String> {
        let ctrl = self.controller.lock().await;
        if !ctrl.is_session_active().await {
            return Some(format!(
                "No active Claude session. Use /claude to start one, or type a builtin command like /help.\n\nYou said: {}",
                message
            ));
        }

        match ctrl.send_message(message).await {
            Ok(()) => None, // Response will come through event channel
            Err(e) => Some(format!("Failed to send message: {}", e)),
        }
    }
}
