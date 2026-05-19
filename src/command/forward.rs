use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::t_fmt;

pub struct ForwardCommand {
    controller: Arc<Mutex<ClaudeController>>,
}

impl ForwardCommand {
    pub fn new(controller: Arc<Mutex<ClaudeController>>) -> Self {
        Self { controller }
    }

    /// Handle regular messages - forward to Claude
    pub async fn handle_regular(&self, message: &str) -> Option<String> {
        let ctrl = self.controller.lock().await;
        if !ctrl.is_session_active().await {
            return Some(t_fmt!("forward.no_session", MSG = message));
        }

        match ctrl.send_message(message).await {
            Ok(()) => None, // Response will come through event channel
            Err(e) => Some(t_fmt!("forward.failed_send", ERR = e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::ClaudeConfig;

    fn setup() -> ForwardCommand {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
        ForwardCommand::new(controller)
    }

    #[tokio::test]
    async fn test_regular_message_returns_prompt_when_inactive() {
        let forward = setup();
        let response = forward.handle_regular("hello").await;
        assert!(response.is_some());
        let text = response.unwrap();
        assert!(text.contains("No active Claude session"));
        assert!(text.contains("hello"));
    }
}
