use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::builtin::BuiltinCommands;
use crate::command::forward::ForwardCommand;
use crate::config::model::GatewayConfig;

pub struct CommandRouter {
    builtin: BuiltinCommands,
    forward: ForwardCommand,
    controller: Arc<Mutex<ClaudeController>>,
}

impl CommandRouter {
    pub fn new(controller: Arc<Mutex<ClaudeController>>, default_dir: &str) -> Self {
        Self {
            builtin: BuiltinCommands::new(controller.clone(), default_dir),
            forward: ForwardCommand::new(controller.clone()),
            controller,
        }
    }

    /// Handle a user message. Returns Some(response) if the message was handled internally.
    pub async fn handle(&self, message: &str) -> Option<String> {
        let trimmed = message.trim();

        // Check if we are in Claude session mode
        let session_active = {
            let ctrl = self.controller.lock().await;
            ctrl.is_session_active().await
        };

        if session_active {
            // In Claude mode: /quit exits the session, /claude restarts it.
            // Everything else (including /help, /cd, raw text, slash commands)
            // is forwarded directly to Claude.
            match trimmed {
                "/quit" | "/claude" => {
                    return self.builtin.handle(trimmed).await;
                }
                _ => {
                    return self.forward.handle_regular(trimmed).await;
                }
            }
        }

        // Session inactive: normal gateway command routing
        if let Some(response) = self.builtin.handle(trimmed).await {
            return Some(response);
        }

        // Regular message: no active session
        self.forward.handle_regular(trimmed).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{ClaudeConfig, GatewayConfig};

    fn setup() -> CommandRouter {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config)));
        CommandRouter::new(controller, "~/Workspace")
    }

    #[tokio::test]
    async fn test_help_handled_by_builtin() {
        let router = setup();
        let response = router.handle("/help").await;
        assert!(response.is_some());
        let text = response.unwrap();
        assert!(text.contains("/help"));
    }

    #[tokio::test]
    async fn test_pwd_handled_by_builtin() {
        let router = setup();
        let response = router.handle("/pwd").await;
        assert!(response.is_some());
        let text = response.unwrap();
        assert!(text.contains("Current directory"));
    }

    #[tokio::test]
    async fn test_slash_command_falls_through_when_inactive() {
        // /clear (or any /cmd) falls through to forward
        let router = setup();
        let response = router.handle("/clear").await;
        assert!(response.is_some());
        let text = response.unwrap();
        assert!(text.contains("No active Claude session"));
    }

    #[tokio::test]
    async fn test_regular_text_forwarded_when_inactive() {
        let router = setup();
        let response = router.handle("hello world").await;
        assert!(response.is_some());
        let text = response.unwrap();
        assert!(text.contains("No active Claude session"));
        assert!(text.contains("hello world"));
    }

    #[tokio::test]
    async fn test_unknown_command_falls_through() {
        let router = setup();
        let response = router.handle("/unknown_command_xyz").await;
        assert!(response.is_some());
        let text = response.unwrap();
        assert!(text.contains("No active Claude session"));
        assert!(text.contains("/unknown_command_xyz"));
    }
}
