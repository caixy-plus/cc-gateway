use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::claude::controller::ClaudeController;

pub struct BuiltinCommands {
    controller: Arc<Mutex<ClaudeController>>,
}

impl BuiltinCommands {
    pub fn new(controller: Arc<Mutex<ClaudeController>>) -> Self {
        Self { controller }
    }

    pub async fn handle(&self, message: &str) -> Option<String> {
        let parts: Vec<&str> = message.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| *s).unwrap_or("");

        match cmd {
            "/help" => Some(self.help()),
            "/cc-quit" | "/quit" => Some(self.quit().await),
            "/cd" => Some(self.cd(arg).await),
            "/claude" => Some(self.claude().await),
            "/pwd" => Some(self.pwd().await),
            "/model" => Some(self.model(arg).await),
            "/status" => Some(self.status().await),
            _ => None,
        }
    }

    fn help(&self) -> String {
        r#"cc-gateway commands:
  /help          Show this help
  /cc-quit       Quit current Claude session
  /cd <path>     Change working directory and restart Claude
  /claude        Start or restart Claude session
  /pwd           Show current working directory
  /model <model> Switch Claude model
  /status        Show gateway status
  /cc/<cmd>      Forward a slash command to Claude (e.g. /cc/clear)

Any other text is sent directly to Claude Code."#
            .to_string()
    }

    async fn quit(&self) -> String {
        let ctrl = self.controller.lock().await;
        match ctrl.stop_session().await {
            Ok(()) => "Claude session stopped.".to_string(),
            Err(e) => format!("Failed to stop session: {}", e),
        }
    }

    async fn cd(&self, path: &str) -> String {
        if path.is_empty() {
            return "Usage: /cd <path>".to_string();
        }

        let expanded = shellexpand::tilde(path).to_string();
        let abs_path = std::path::PathBuf::from(&expanded);
        let abs_path = match abs_path.canonicalize() {
            Ok(p) => p,
            Err(_) => {
                // Try with default dir prefix if relative
                let default = std::path::PathBuf::from(
                    shellexpand::tilde("~/Workspace").to_string());
                let combined = default.join(&expanded);
                match combined.canonicalize() {
                    Ok(p) => p,
                    Err(e) => return format!("Invalid path: {} ({})", path, e),
                }
            }
        };

        if !abs_path.is_dir() {
            return format!("Not a directory: {}", abs_path.display());
        }

        let path_str = abs_path.to_string_lossy().to_string();
        let ctrl = self.controller.lock().await;
        match ctrl.set_work_dir(path_str.clone()).await {
            Ok(()) => {
                format!("Working directory changed to: {}", path_str)
            }
            Err(e) => format!("Failed to change directory: {}", e),
        }
    }

    async fn claude(&self) -> String {
        let ctrl = self.controller.lock().await;
        let work_dir = ctrl.get_work_dir().await;
        let dir = if work_dir.is_empty() {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string())
        } else {
            work_dir
        };

        match ctrl.start_session(dir.clone()).await {
            Ok(()) => format!("Claude session started in: {}", dir),
            Err(e) => format!("Failed to start Claude: {}", e),
        }
    }

    async fn pwd(&self) -> String {
        let ctrl = self.controller.lock().await;
        let work_dir = ctrl.get_work_dir().await;
        if work_dir.is_empty() {
            match std::env::current_dir() {
                Ok(p) => format!("Current directory: {}", p.display()),
                Err(e) => format!("Error: {}", e),
            }
        } else {
            format!("Current directory: {}", work_dir)
        }
    }

    async fn model(&self, model: &str) -> String {
        if model.is_empty() {
            let ctrl = self.controller.lock().await;
            return format!(
                "Current model: {}. Usage: /model <sonnet|opus|haiku|...\u003e",
                "(default)"
            );
        }

        let mut ctrl = self.controller.lock().await;
        match ctrl.set_model(model.to_string()).await {
            Ok(()) => format!("Model switched to: {}", model),
            Err(e) => format!("Failed to switch model: {}", e),
        }
    }

    async fn status(&self) -> String {
        let ctrl = self.controller.lock().await;
        let active = ctrl.is_session_active().await;
        let work_dir = ctrl.get_work_dir().await;
        format!(
            "Status:\n  Session: {}\n  Work dir: {}",
            if active { "active" } else { "inactive" },
            if work_dir.is_empty() { "(not set)" } else { &work_dir }
        )
    }
}
