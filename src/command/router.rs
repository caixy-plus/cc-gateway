use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::builtin::BuiltinCommands;
use crate::command::forward::ForwardCommand;
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
            // In Claude mode: /quit and /show-thinking-toggle are handled locally.
            // Everything else (including /help, /cd, /ll, /claude, raw text,
            // slash commands) is forwarded directly to Claude.
            match trimmed {
                "/quit" | "/show-thinking-toggle" | "/show-thinking" | "/hide-thinking"
                | "/claude-history" | "/claude-resume" => {
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
    use crate::config::model::ClaudeConfig;

    fn setup() -> CommandRouter {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
        CommandRouter::new(controller, "~")
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

    #[tokio::test]
    async fn test_quit_handled_locally_when_session_active() {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
        let router = CommandRouter::new(controller.clone(), "~");

        {
            let ctrl = controller.lock().await;
            ctrl.inject_dummy_session().await.unwrap();
        }

        let response = router.handle("/quit").await;
        assert!(response.is_some(), "/quit should be handled locally when session is active");
    }

    #[tokio::test]
    async fn test_ll_forwarded_when_session_active() {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
        let router = CommandRouter::new(controller.clone(), "~");

        {
            let ctrl = controller.lock().await;
            ctrl.inject_dummy_session().await.unwrap();
        }

        let response = router.handle("/ll").await;
        assert!(
            response.is_none(),
            "/ll should be forwarded to Claude when session is active, got: {:?}",
            response
        );

        {
            let ctrl = controller.lock().await;
            let _ = ctrl.stop_session().await;
        }
    }

    #[tokio::test]
    async fn test_claude_forwarded_when_session_active() {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
        let router = CommandRouter::new(controller.clone(), "~");

        {
            let ctrl = controller.lock().await;
            ctrl.inject_dummy_session().await.unwrap();
        }

        let response = router.handle("/claude").await;
        assert!(
            response.is_none(),
            "/claude should be forwarded to Claude when session is active, got: {:?}",
            response
        );

        {
            let ctrl = controller.lock().await;
            let _ = ctrl.stop_session().await;
        }
    }

    #[tokio::test]
    async fn test_text_forwarded_when_session_active() {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
        let router = CommandRouter::new(controller.clone(), "~");

        {
            let ctrl = controller.lock().await;
            ctrl.inject_dummy_session().await.unwrap();
        }

        let response = router.handle("hello world").await;
        assert!(
            response.is_none(),
            "regular text should be forwarded to Claude when session is active, got: {:?}",
            response
        );

        {
            let ctrl = controller.lock().await;
            let _ = ctrl.stop_session().await;
        }
    }
}
