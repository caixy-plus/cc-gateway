use anyhow::Result;
use std::path::Path;
use tracing::{debug, info, warn};

use crate::ai::client::AiClient;
use crate::config::model::AiConfig;

pub struct IntentAnalyzer {
    ai: Option<AiClient>,
    scan_dirs: Vec<String>,
}

impl IntentAnalyzer {
    pub fn new(ai_config: &AiConfig, scan_dirs: Vec<String>) -> Self {
        let ai = if ai_config.enabled {
            Some(AiClient::new(ai_config.clone()))
        } else {
            None
        };
        Self { ai, scan_dirs }
    }

    /// Analyze user message to detect if they want to work on a local project.
    /// Returns the project path if detected.
    pub async fn detect_project(&self, message: &str) -> Option<String> {
        if self.ai.is_none() {
            return self.fuzzy_scan_project(message);
        }

        let ai = self.ai.as_ref().unwrap();
        let system = r#"You are a helpful assistant. The user is talking to a gateway that controls Claude Code.
Analyze their message. If they want to work on a local project, respond ONLY with the project directory name or a short path.
If no project is mentioned, respond with "NONE".
Examples:
- "帮我开发本地那个支付项目" -> "payment"
- "fix the gateway bug" -> "gateway"
- "你好" -> "NONE""#;

        match ai.chat(system, message).await {
            Ok(response) => {
                let trimmed = response.trim();
                if trimmed == "NONE" || trimmed.is_empty() {
                    None
                } else {
                    self.resolve_project_path(trimmed)
                }
            }
            Err(e) => {
                warn!("AI intent analysis failed: {}, falling back to fuzzy scan", e);
                self.fuzzy_scan_project(message)
            }
        }
    }

    fn fuzzy_scan_project(&self, message: &str) -> Option<String> {
        let lower_msg = message.to_lowercase();
        for dir in &self.scan_dirs {
            let expanded = shellexpand::tilde(dir).to_string();
            let path = Path::new(&expanded);
            if !path.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    if let Some(name) = entry.file_name().to_str() {
                        let lower_name = name.to_lowercase();
                        if lower_msg.contains(&lower_name) {
                            return Some(entry.path().to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
        None
    }

    fn resolve_project_path(&self, name: &str) -> Option<String> {
        for dir in &self.scan_dirs {
            let expanded = shellexpand::tilde(dir).to_string();
            let path = Path::new(&expanded).join(name);
            if path.is_dir() {
                return Some(path.to_string_lossy().to_string());
            }
        }
        self.fuzzy_scan_project(name)
    }
}
