use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::builtin::BuiltinCommands;
use crate::command::forward::ForwardCommand;

pub struct CommandRouter {
    builtin: BuiltinCommands,
    forward: ForwardCommand,
}

impl CommandRouter {
    pub fn new(controller: Arc<Mutex<ClaudeController>>) -> Self {
        Self {
            builtin: BuiltinCommands::new(controller.clone()),
            forward: ForwardCommand::new(controller),
        }
    }

    /// Handle a user message. Returns Some(response) if the message was handled internally.
    pub async fn handle(&self, message: &str) -> Option<String> {
        let trimmed = message.trim();

        // Check for gateway builtin commands
        if let Some(response) = self.builtin.handle(trimmed).await {
            return Some(response);
        }

        // Check for /cc/ prefix to forward to Claude
        if trimmed.starts_with("/cc/") {
            return self.forward.handle(trimmed).await;
        }

        // Regular message: forward to Claude if session is active
        self.forward.handle_regular(trimmed).await
    }
}
