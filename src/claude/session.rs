use anyhow::{Context, Result};
use serde_json;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::claude::protocol::{InputMessage, OutputEvent};
use crate::config::model::ClaudeConfig;

pub struct ClaudeSession {
    child: Child,
    stdin: tokio::process::ChildStdin,
    #[allow(dead_code)]
    work_dir: String,
}

fn resolve_cli_path(config_path: &str) -> String {
    // If it's an absolute path and exists, use it directly
    let path = std::path::PathBuf::from(config_path);
    if path.is_absolute() && path.exists() {
        return config_path.to_string();
    }

    // Try to resolve via the user's shell (handles shell functions/aliases)
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_cmd = format!("command -v {} 2>/dev/null", config_path);
    if let Ok(output) = std::process::Command::new(&shell)
        .arg("-c")
        .arg(&shell_cmd)
        .output()
    {
        let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !resolved.is_empty() && std::path::Path::new(&resolved).exists() {
            return resolved;
        }
    }

    // Try common fallback paths
    let home = dirs::home_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
    let fallbacks = [
        format!("{}/.local/bin/{}", home, config_path),
        format!("/usr/local/bin/{}", config_path),
        format!("/opt/homebrew/bin/{}", config_path),
        format!("/usr/bin/{}", config_path),
    ];
    for fb in &fallbacks {
        if std::path::Path::new(fb).exists() {
            return fb.clone();
        }
    }

    config_path.to_string()
}

impl ClaudeSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &ClaudeConfig,
        event_tx: mpsc::UnboundedSender<OutputEvent>,
    ) -> Result<Self> {
        let cli_path = resolve_cli_path(&config.cli_path);

        let mut args = vec![
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--permission-prompt-tool".to_string(),
            "stdio".to_string(),
            "--verbose".to_string(),
        ];

        // Append default args from config
        if !config.default_args.is_empty() {
            for arg in config.default_args.split_whitespace() {
                args.push(arg.to_string());
            }
        }

        // Append any extra args passed via /claude <cmd>
        for arg in extra_args {
            args.push(arg);
        }

        info!(
            "Starting Claude session: {} {:?} in {}",
            cli_path, args, work_dir
        );

        let mut cmd = Command::new(&cli_path);
        cmd.args(&args)
            .current_dir(&work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Filter out CLAUDECODE env var to prevent nested session detection
        let env_vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k != "CLAUDECODE")
            .collect();
        cmd.env_clear();
        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn Claude Code. Is '{}' installed and on PATH? Tried '{}'.",
                config.cli_path, cli_path
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .context("Failed to open stdin pipe")?;
        let stdout = child
            .stdout
            .take()
            .context("Failed to open stdout pipe")?;
        let stderr = child.stderr.take().context("Failed to open stderr pipe")?;

        // Spawn stderr reader
        tokio::spawn(Self::stderr_reader(stderr));

        // Spawn stdout reader
        let tx = event_tx.clone();
        tokio::spawn(Self::stdout_reader(stdout, tx));

        Ok(Self {
            child,
            stdin,
            work_dir,
        })
    }

    pub async fn send(&mut self, msg: InputMessage) -> Result<()> {
        let json = serde_json::to_string(&msg)?;
        debug!("→ Claude: {}", json);
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub async fn stop(mut self) -> Result<()> {
        // Close stdin to signal EOF
        drop(self.stdin);

        // Wait for graceful shutdown
        let timeout = tokio::time::Duration::from_secs(30);
        match tokio::time::timeout(timeout, self.child.wait()).await {
            Ok(Ok(status)) => {
                info!("Claude session exited with status: {}", status);
            }
            Ok(Err(e)) => {
                warn!("Claude session wait error: {}", e);
            }
            Err(_) => {
                warn!("Claude session graceful shutdown timed out, killing");
                let _ = self.child.kill().await;
            }
        }
        Ok(())
    }

    async fn stdout_reader(
        stdout: tokio::process::ChildStdout,
        tx: mpsc::UnboundedSender<OutputEvent>,
    ) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            debug!("← Claude: {}", line);

            match serde_json::from_str::<OutputEvent>(&line) {
                Ok(event) => {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(e) => {
                    debug!("Non-JSON line from Claude: {} (err: {})", line, e);
                }
            }
        }

        info!("Claude stdout reader ended");
    }

    async fn stderr_reader(stderr: tokio::process::ChildStderr) {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                debug!("Claude stderr: {}", line);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_cli_path_absolute_exists() {
        // Test with a known absolute path that exists
        let path = std::env::current_exe().unwrap();
        let path_str = path.to_string_lossy().to_string();
        let resolved = resolve_cli_path(&path_str);
        assert_eq!(resolved, path_str);
    }

    #[test]
    fn test_resolve_cli_path_fallback() {
        // Test with a non-existent path that won't resolve via shell
        let resolved = resolve_cli_path("this_binary_definitely_does_not_exist_12345");
        assert_eq!(resolved, "this_binary_definitely_does_not_exist_12345");
    }

    #[test]
    fn test_resolve_cli_path_resolves_shell_command() {
        // 'ls' should exist on most Unix systems
        #[cfg(unix)]
        {
            let resolved = resolve_cli_path("ls");
            assert_ne!(resolved, "ls"); // Should be resolved to absolute path
            assert!(resolved.starts_with('/'));
        }
    }
}
