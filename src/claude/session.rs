use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::claude::mcp_server::McpContext;
use crate::claude::protocol::{InputMessage, OutputEvent};
use crate::config::model::ClaudeConfig;

#[derive(Debug, Deserialize)]
struct ClaudeSessionFile {
    #[serde(rename = "sessionId")]
    session_id: String,
}

pub struct ClaudeSession {
    child: Child,
    stdin: tokio::process::ChildStdin,
    #[allow(dead_code)]
    work_dir: String,
    mcp_config_path: Option<PathBuf>,
}

pub(crate) fn resolve_cli_path(config_path: &str) -> String {
    // If it's an absolute path and exists, use it directly
    let path = std::path::PathBuf::from(config_path);
    if path.is_absolute() && path.exists() {
        return config_path.to_string();
    }

    // On Windows, try resolving via `where` (handles .exe, .cmd, .bat, etc.)
    #[cfg(windows)]
    {
        for search_name in [config_path, &format!("{}.exe", config_path)] {
            if let Ok(output) = std::process::Command::new("cmd")
                .args(["/C", "where", search_name])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // `where` may return multiple lines (e.g. npm installs both
                // a shell script and a .cmd). Pick a line with a known
                // Windows executable extension first.
                let resolved = stdout
                    .lines()
                    .find(|line| {
                        let lower = line.to_lowercase();
                        lower.ends_with(".exe")
                            || lower.ends_with(".cmd")
                            || lower.ends_with(".bat")
                    })
                    .or_else(|| stdout.lines().next())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !resolved.is_empty() && std::path::Path::new(&resolved).exists() {
                    return resolved;
                }
            }
        }
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
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
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

        // Append resume session id if provided
        if let Some(ref sid) = resume_session_id {
            args.push("--resume".to_string());
            args.push(sid.clone());
        }

        // Generate MCP config file if context is provided
        let mcp_config_path = if let Some(ref mcp_ctx) = mcp_context {
            let config_path = std::env::temp_dir()
                .join(format!("cc-gateway-mcp-{}.json", uuid::Uuid::new_v4()));
            let config_json = serde_json::json!({
                "mcpServers": {
                    "cc-gateway": {
                        "command": std::env::current_exe().unwrap_or_else(|_| PathBuf::from("cc-gateway")),
                        "args": ["_mcp-server"],
                        "env": {
                            "CC_GATEWAY_FEISHU_APP_ID": mcp_ctx.feishu_app_id,
                            "CC_GATEWAY_FEISHU_APP_SECRET": mcp_ctx.feishu_app_secret,
                            "CC_GATEWAY_FEISHU_CHAT_ID": mcp_ctx.chat_id,
                            "CC_GATEWAY_FEISHU_RECEIVE_ID_TYPE": mcp_ctx.receive_id_type,
                        }
                    }
                }
            });
            let content = serde_json::to_string_pretty(&config_json)?;
            tokio::fs::write(&config_path, &content).await?;
            info!("Generated MCP config at {:?}", config_path);
            args.push("--mcp-config".to_string());
            args.push(config_path.to_string_lossy().to_string());
            Some(config_path)
        } else {
            None
        };

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

        let pid = child.id();

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

        // Try to extract Claude session id from the sessions file
        let claude_session_id = if let Some(pid) = pid {
            Self::extract_session_id_with_retry(pid).await
        } else {
            None
        };

        Ok((
            Self {
                child,
                stdin,
                work_dir,
                mcp_config_path,
            },
            claude_session_id,
        ))
    }

    async fn extract_session_id_with_retry(pid: u32) -> Option<String> {
        let session_path = dirs::home_dir()
            .map(|h| h.join(".claude").join("sessions").join(format!("{}.json", pid)))?;

        // Initial delay before first attempt — Claude needs time to create the session file
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        for attempt in 0..15 {
            if session_path.exists() {
                match tokio::fs::read_to_string(&session_path).await {
                    Ok(content) => {
                        if let Ok(session_file) = serde_json::from_str::<ClaudeSessionFile>(&content) {
                            info!("Extracted Claude session id: {} from pid {}", session_file.session_id, pid);
                            return Some(session_file.session_id);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read Claude session file {:?}: {}", session_path, e);
                    }
                }
            }
            if attempt < 14 {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        warn!("Could not extract Claude session id for pid {} after retries", pid);
        None
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
        let _ = self.child.kill().await;
        let _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            self.child.wait(),
        )
        .await;

        // Clean up MCP config file
        if let Some(ref path) = self.mcp_config_path {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to remove MCP config file {:?}: {}", path, e);
            }
        }

        info!("Claude session stopped");
        Ok(())
    }

    /// Check whether the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                warn!("Claude process exited with status: {}", status);
                false
            }
            Err(e) => {
                warn!("Failed to check Claude process status: {}", e);
                false
            }
        }
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
impl ClaudeSession {
    /// Create a dummy session for testing that keeps a sleep process alive.
    pub async fn dummy_for_test() -> Result<Self> {
        let mut child = Command::new("sleep")
            .arg("3600")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn dummy sleep process for test")?;

        let stdin = child
            .stdin
            .take()
            .context("Failed to open stdin pipe for dummy session")?;

        Ok(Self {
            child,
            stdin,
            work_dir: ".".to_string(),
            mcp_config_path: None,
        })
    }
}
