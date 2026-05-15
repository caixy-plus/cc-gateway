use anyhow::{Context, Result};
use serde_json;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::claude::protocol::{InputMessage, OutputEvent};
use crate::config::model::ClaudeConfig;

pub struct ClaudeSession {
    child: Child,
    stdin: tokio::process::ChildStdin,
    event_tx: mpsc::UnboundedSender<OutputEvent>,
    work_dir: String,
}

impl ClaudeSession {
    pub async fn spawn(
        work_dir: String,
        config: &ClaudeConfig,
        event_tx: mpsc::UnboundedSender<OutputEvent>,
    ) -> Result<Self> {
        let mut args = vec![
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--permission-prompt-tool".to_string(),
            "stdio".to_string(),
            "--verbose".to_string(),
        ];

        if !config.mode.is_empty() && config.mode != "default" {
            args.push("--permission-mode".to_string());
            args.push(config.mode.clone());
        }

        if !config.model.is_empty() {
            args.push("--model".to_string());
            args.push(config.model.clone());
        }

        if !config.reasoning_effort.is_empty() {
            args.push("--effort".to_string());
            args.push(config.reasoning_effort.clone());
        }

        if !config.system_prompt.is_empty() {
            args.push("--system-prompt".to_string());
            args.push(config.system_prompt.clone());
        }

        if !config.allowed_tools.is_empty() {
            args.push("--allowedTools".to_string());
            args.push(config.allowed_tools.join(","));
        }

        if !config.disallowed_tools.is_empty() {
            args.push("--disallowedTools".to_string());
            args.push(config.disallowed_tools.join(","));
        }

        info!(
            "Starting Claude session: {} {:?} in {}",
            config.cli_path, args, work_dir
        );

        let mut cmd = Command::new(&config.cli_path);
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
                "Failed to spawn Claude Code. Is '{}' installed and on PATH?",
                config.cli_path
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
            event_tx,
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

    pub fn work_dir(&self) -> &str {
        &self.work_dir
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
