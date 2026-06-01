use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::agent::mcp_attach::build_claude_mcp_servers_object;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;
use crate::runtime::protocol::{InputMessage, OutputEvent};

#[derive(Debug, Deserialize)]
struct AgentSessionFile {
    #[serde(rename = "sessionId")]
    session_id: String,
}

pub struct StreamJsonSession {
    child: Child,
    stdin: tokio::process::ChildStdin,
    work_dir: String,
    mcp_config_path: Option<PathBuf>,
    stderr_lines: Arc<Mutex<Vec<String>>>,
    /// Retained so `/clear` can respawn Claude while keeping the same event bridge alive.
    output_tx: mpsc::UnboundedSender<OutputEvent>,
}

pub(crate) fn resolve_cli_path(config_path: &str) -> String {
    // If it's an absolute path and exists, use it directly
    let path = std::path::PathBuf::from(config_path);
    if path.is_absolute() && path.exists() {
        return config_path.to_string();
    }

    // Walk PATH before shell lookup so test fakes (prepended to PATH) win over
    // global installs and zsh functions that `command -v` may prefer.
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join(config_path);
            if candidate.is_file() {
                return candidate.to_string_lossy().to_string();
            }
            #[cfg(windows)]
            {
                let candidate_exe = dir.join(format!("{}.exe", config_path));
                if candidate_exe.is_file() {
                    return candidate_exe.to_string_lossy().to_string();
                }
            }
        }
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
    let home = dirs::home_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let fallbacks = [
        format!("{}/.local/bin/{}", home, config_path),
        format!("{}/{}", home, config_path),
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

impl StreamJsonSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
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
            let config_path =
                std::env::temp_dir().join(format!("cc-gateway-mcp-{}.json", uuid::Uuid::new_v4()));
            let config_json = serde_json::json!({
                "mcpServers": build_claude_mcp_servers_object(mcp_ctx)?,
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

        // Append any extra args passed via /agent <args>
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

        let stdin = child.stdin.take().context("Failed to open stdin pipe")?;
        let stdout = child.stdout.take().context("Failed to open stdout pipe")?;
        let stderr = child.stderr.take().context("Failed to open stderr pipe")?;

        // Spawn stderr reader
        let stderr_lines = Arc::new(Mutex::new(Vec::new()));
        tokio::spawn(Self::stderr_reader(stderr, stderr_lines.clone()));

        // Spawn stdout reader
        let tx = event_tx.clone();
        tokio::spawn(Self::stdout_reader(stdout, tx));

        // Try to extract Claude session id from the sessions file
        let provider_session_id = if let Some(pid) = pid {
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
                stderr_lines,
                output_tx: event_tx,
            },
            provider_session_id,
        ))
    }

    /// Start a fresh Claude subprocess (no `--resume`) and return the new provider session id.
    pub async fn restart_fresh(
        &mut self,
        extra_args: Vec<String>,
        config: &AgentConfig,
        mcp_context: Option<McpContext>,
    ) -> Result<Option<String>> {
        let _ = self.child.kill().await;
        if let Some(ref path) = self.mcp_config_path {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to remove MCP config file {:?}: {}", path, e);
            }
        }

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
        if !config.default_args.is_empty() {
            for arg in config.default_args.split_whitespace() {
                args.push(arg.to_string());
            }
        }

        let mcp_config_path = if let Some(ref mcp_ctx) = mcp_context {
            let config_path =
                std::env::temp_dir().join(format!("cc-gateway-mcp-{}.json", uuid::Uuid::new_v4()));
            let config_json = serde_json::json!({
                "mcpServers": build_claude_mcp_servers_object(mcp_ctx)?,
            });
            let content = serde_json::to_string_pretty(&config_json)?;
            tokio::fs::write(&config_path, &content).await?;
            args.push("--mcp-config".to_string());
            args.push(config_path.to_string_lossy().to_string());
            Some(config_path)
        } else {
            None
        };

        for arg in extra_args {
            args.push(arg);
        }

        info!(
            "Restarting Claude session (fresh): {} {:?} in {}",
            cli_path, args, self.work_dir
        );

        let mut cmd = Command::new(&cli_path);
        cmd.args(&args)
            .current_dir(&self.work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let env_vars: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k != "CLAUDECODE")
            .collect();
        cmd.env_clear();
        for (k, v) in env_vars {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to respawn Claude Code. Is '{}' installed and on PATH? Tried '{}'.",
                config.cli_path, cli_path
            )
        })?;

        let pid = child.id();
        let stdin = child.stdin.take().context("Failed to open stdin pipe")?;
        let stdout = child.stdout.take().context("Failed to open stdout pipe")?;
        let stderr = child.stderr.take().context("Failed to open stderr pipe")?;

        if let Ok(mut lines) = self.stderr_lines.lock() {
            lines.clear();
        }
        tokio::spawn(Self::stderr_reader(stderr, self.stderr_lines.clone()));
        tokio::spawn(Self::stdout_reader(stdout, self.output_tx.clone()));

        self.child = child;
        self.stdin = stdin;
        self.mcp_config_path = mcp_config_path;

        let provider_session_id = if let Some(pid) = pid {
            Self::extract_session_id_with_retry(pid).await
        } else {
            None
        };
        Ok(provider_session_id)
    }

    async fn extract_session_id_with_retry(pid: u32) -> Option<String> {
        let session_path = dirs::home_dir().map(|h| {
            h.join(".claude")
                .join("sessions")
                .join(format!("{}.json", pid))
        })?;

        // Initial delay before first attempt — Claude needs time to create the session file
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;

        for attempt in 0..15 {
            if session_path.exists() {
                match tokio::fs::read_to_string(&session_path).await {
                    Ok(content) => {
                        if let Ok(session_file) = serde_json::from_str::<AgentSessionFile>(&content)
                        {
                            info!(
                                "Extracted Claude session id: {} from pid {}",
                                session_file.session_id, pid
                            );
                            return Some(session_file.session_id);
                        }
                    }
                    Err(e) => {
                        warn!(
                            "Failed to read Claude session file {:?}: {}",
                            session_path, e
                        );
                    }
                }
            }
            if attempt < 14 {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }

        warn!(
            "Could not extract Claude session id for pid {} after retries",
            pid
        );
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

        // Clean up MCP config file
        if let Some(ref path) = self.mcp_config_path {
            if let Err(e) = std::fs::remove_file(path) {
                warn!("Failed to remove MCP config file {:?}: {}", path, e);
            }
        }

        info!("Claude session stopped");
        Ok(())
    }

    pub async fn force_stop(self) -> Result<()> {
        // Same as stop() today: immediately kill the process.
        // Kept separate so higher layers can explicitly choose "force".
        self.stop().await
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

    pub fn recent_stderr(&self) -> String {
        self.stderr_lines
            .lock()
            .map(|lines| lines.join("\n"))
            .unwrap_or_default()
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

    async fn stderr_reader(
        stderr: tokio::process::ChildStderr,
        stderr_lines: Arc<Mutex<Vec<String>>>,
    ) {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                if let Ok(mut lines) = stderr_lines.lock() {
                    lines.push(line.clone());
                    if lines.len() > 20 {
                        lines.remove(0);
                    }
                }
                debug!("Claude stderr: {}", line);
            }
        }
    }
}
