#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// Returns the default system prompt for cc-gateway.
///
/// Describes the gateway's purpose, available commands, and MCP Bash tool availability.
pub fn load_default_prompt() -> String {
    r#"You are interacting with cc-gateway, a gateway for controlling Claude Code via Feishu/Lark and CLI.

Purpose:
- cc-gateway bridges external chat platforms (Feishu/Lark) and local CLI to Claude Code sessions.
- It manages working directories, skills, permissions, and tool execution on behalf of the user.

Available commands:
  /cd <path>     Change the working directory and restart the Claude session
  /pwd           Show the current working directory
  /ll            List files in the current directory (ls -l)
  /skill [name]  List available skills or load a specific skill into the session
  /allow         Approve a pending tool permission request
  /deny [reason] Deny a pending tool permission request

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_default_prompt_contains_key_sections() {
        let prompt = load_default_prompt();
        assert!(prompt.contains("cc-gateway"));
        assert!(prompt.contains("/cd"));
        assert!(prompt.contains("/pwd"));
        assert!(prompt.contains("/ll"));
        assert!(prompt.contains("/skill"));
        assert!(prompt.contains("/allow"));
        assert!(prompt.contains("/deny"));
        assert!(prompt.contains("MCP Bash tool"));
        assert!(prompt.contains("safety"));
    }

    #[test]
    fn test_load_default_prompt_is_non_empty() {
        let prompt = load_default_prompt();
        assert!(!prompt.is_empty());
    }

    #[test]
    fn test_load_prompt_from_file_success() {
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "Custom prompt content").unwrap();
        let path = temp.path();
        let result = load_prompt_from_file(path);
        assert!(result.is_some());
        assert!(result.unwrap().contains("Custom prompt content"));
    }

    #[test]
    fn test_load_prompt_from_file_missing() {
        let result = load_prompt_from_file("/nonexistent/path/to/prompt.txt");
        assert!(result.is_none());
    }

    #[test]
    fn test_load_prompt_from_file_empty() {
        let temp = NamedTempFile::new().unwrap();
        let result = load_prompt_from_file(temp.path());
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "");
    }
}
