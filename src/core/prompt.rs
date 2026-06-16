#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// Returns the default system prompt for cc-gateway.
///
/// Describes the gateway's purpose, available commands, and MCP Bash tool availability.
pub fn load_default_prompt() -> String {
    r#"You are interacting with cc-gateway, a gateway for controlling local agent CLIs via Feishu/Lark, Telegram, and WebUI.

Purpose:
- cc-gateway bridges external chat platforms and WebUI to agent CLI sessions on the host.
- It manages working directories, permissions, and tool execution on behalf of the user.

Available commands:
  /agent [args]  Start or restart the active agent session
  /cd <path>     Change the working directory
  /pwd           Show the current working directory
  /ll            List subdirectories under the workspace

MCP Bash tool:
- The Bash tool is available for executing shell commands through the gateway.
- Execution is subject to safety restrictions configured by the administrator.
- Depending on the current safety mode, commands may be whitelisted, blacklisted, or require confirmation before execution.
- Long-running commands are automatically killed after the configured timeout.
- Excessively large outputs are truncated to protect the session.

Respond helpfully and concisely."#
        .to_string()
}

/// Load a custom prompt from a file.
///
/// Returns `Some(String)` if the file exists and is readable,
/// otherwise returns `None`.
pub fn load_prompt_from_file<P: AsRef<Path>>(path: P) -> Option<String> {
    match fs::read_to_string(path.as_ref()) {
        Ok(content) => Some(content),
        Err(e) => {
            tracing::warn!(
                "Failed to read prompt from {}: {}",
                path.as_ref().display(),
                e
            );
            None
        }
    }
}
