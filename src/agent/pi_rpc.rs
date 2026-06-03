use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use crate::agent::event::AgentEvent;
use crate::config::model::AgentConfig;

type PendingRpc = std::collections::HashMap<String, oneshot::Sender<Value>>;

pub struct PiRpcSession {
    child: Child,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    is_busy: Arc<AtomicBool>,
    stderr_lines: Arc<StdMutex<Vec<String>>>,
    pending_rpc: Arc<Mutex<PendingRpc>>,
}

impl PiRpcSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        _resume_session_id: Option<String>,
    ) -> Result<(Self, Option<String>)> {
        let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);

        let mut args = vec!["--mode".to_string(), "rpc".to_string()];

        // Append default args from config
        if !config.default_args.is_empty() {
            for arg in config.default_args.split_whitespace() {
                args.push(arg.to_string());
            }
        }

        // Append extra args passed via /agent <args>
        for arg in extra_args {
            args.push(arg);
        }

        info!(
            "Starting Pi RPC session: {} {:?} in {}",
            cli_path, args, work_dir
        );

        let mut cmd = Command::new(&cli_path);
        cmd.args(&args)
            .current_dir(&work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Pass through environment, filtering out provider-specific vars
        cmd.env_clear();
        for (k, v) in crate::agent::passthrough_env() {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn Pi. Is '{}' installed and on PATH? Tried '{}'.",
                config.cli_path, cli_path
            )
        })?;

        let stdin = child.stdin.take().context("Failed to open stdin pipe")?;
        let stdout = child.stdout.take().context("Failed to open stdout pipe")?;
        let stderr = child.stderr.take().context("Failed to open stderr pipe")?;

        let stdin = Arc::new(Mutex::new(stdin));
        let is_busy = Arc::new(AtomicBool::new(false));
        let pending_rpc: Arc<Mutex<PendingRpc>> = Arc::new(Mutex::new(std::collections::HashMap::new()));

        // Spawn stderr reader
        let stderr_lines = Arc::new(StdMutex::new(Vec::new()));
        tokio::spawn(Self::stderr_reader(stderr, stderr_lines.clone()));

        // Spawn stdout reader
        let tx = event_tx.clone();
        let busy = is_busy.clone();
        let stdin_clone = stdin.clone();
        let pending = pending_rpc.clone();
        tokio::spawn(Self::stdout_reader(stdout, tx, busy, stdin_clone, pending));

        Ok((
            Self {
                child,
                stdin,
                is_busy,
                stderr_lines,
                pending_rpc,
            },
            None, // Pi RPC doesn't have a persistent session ID concept like Claude
        ))
    }

    pub async fn send_user_message(&self, text: &str) -> Result<()> {
        let mut payload = json!({
            "type": "prompt",
            "message": text,
        });
        // If already busy, use steering behavior
        if self.is_busy.load(Ordering::Relaxed) {
            payload["streamingBehavior"] = json!("steer");
        }
        self.write_json(&payload).await?;
        self.is_busy.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Send abort to interrupt the current agent operation.
    pub async fn send_cancel(&self) -> Result<()> {
        self.write_json(&json!({"type": "abort"})).await?;
        self.is_busy.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// Send permission response for an extension UI request.
    pub async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()> {
        let response = if allow {
            json!({
                "type": "extension_ui_response",
                "id": request_id,
                "confirmed": true
            })
        } else {
            json!({
                "type": "extension_ui_response",
                "id": request_id,
                "cancelled": true
            })
        };
        self.write_json(&response).await
    }

    /// Reset Pi context via `new_session` (no separate provider session id).
    pub async fn new_provider_session(&self) -> Result<Option<String>> {
        self.write_json(&json!({"type": "new_session"})).await?;
        Ok(None)
    }

    pub async fn get_available_models(&self) -> Result<Vec<String>> {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_rpc.lock().await;
            pending.insert(id.clone(), tx);
        }
        self.write_json(&json!({"type": "get_available_models", "id": id}))
            .await?;
        let msg = tokio::time::timeout(std::time::Duration::from_secs(15), rx)
            .await
            .context("Pi get_available_models timed out")?
            .context("Pi get_available_models response channel closed")?;
        let models = msg
            .get("data")
            .and_then(|d| d.get("models"))
            .and_then(|m| m.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| {
                        // docs: Model objects; tolerate string ids too
                        m.get("id")
                            .or_else(|| m.get("modelId"))
                            .or_else(|| m.get("name"))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| m.as_str().map(|s| s.to_string()))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(models)
    }

    pub async fn set_model(&self, provider: &str, model_id: &str) -> Result<()> {
        self.write_json(&json!({
            "type": "set_model",
            "provider": provider,
            "modelId": model_id,
        }))
        .await
    }

    pub async fn stop(mut self) -> Result<()> {
        // Best-effort abort before killing
        let _ = self.write_json(&json!({"type": "abort"})).await;
        crate::agent::acp_client::kill_child_process_tree(&mut self.child).await;
        info!("Pi RPC session stopped");
        Ok(())
    }

    pub async fn force_stop(mut self) -> Result<()> {
        crate::agent::acp_client::kill_child_process_tree(&mut self.child).await;
        info!("Pi RPC session force-stopped");
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                warn!("Pi process exited with status: {}", status);
                false
            }
            Err(e) => {
                warn!("Failed to check Pi process status: {}", e);
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

    async fn write_json(&self, value: &Value) -> Result<()> {
        let line = serde_json::to_string(value)?;
        debug!("→ Pi: {}", line);
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn stdout_reader(
        stdout: tokio::process::ChildStdout,
        tx: mpsc::UnboundedSender<AgentEvent>,
        is_busy: Arc<AtomicBool>,
        _stdin: Arc<Mutex<tokio::process::ChildStdin>>,
        pending_rpc: Arc<Mutex<PendingRpc>>,
    ) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            debug!("← Pi: {}", line);

            let msg: Value = match serde_json::from_str(&line) {
                Ok(val) => val,
                Err(e) => {
                    debug!("Non-JSON line from Pi: {} (err: {})", line, e);
                    continue;
                }
            };

            let event_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

            match event_type {
                "response" => {
                    // Command response — we don't need to handle these beyond
                    // logging, but we can detect get_state responses for session id.
                    if let Some(cmd) = msg.get("command").and_then(|v| v.as_str()) {
                        debug!("Pi response for command: {}", cmd);
                    }
                    if let Some(id) = msg.get("id").and_then(|v| v.as_str()) {
                        if let Some(tx) = pending_rpc.lock().await.remove(id) {
                            let _ = tx.send(msg.clone());
                        }
                    }
                }

                "agent_start" => {
                    debug!("Pi agent started");
                }

                "agent_end" => {
                    debug!("Pi agent ended");
                    is_busy.store(false, Ordering::Relaxed);
                    let _ = tx.send(AgentEvent::Done);
                }

                "message_update" => {
                    if let Some(assistant_event) = msg.get("assistantMessageEvent") {
                        let delta_type = assistant_event
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        match delta_type {
                            "text_delta" => {
                                if let Some(delta) =
                                    assistant_event.get("delta").and_then(|v| v.as_str())
                                {
                                    let _ = tx.send(AgentEvent::Text(delta.to_string()));
                                }
                            }
                            "thinking_delta" => {
                                if let Some(delta) =
                                    assistant_event.get("delta").and_then(|v| v.as_str())
                                {
                                    let _ = tx.send(AgentEvent::Thinking(delta.to_string()));
                                }
                            }
                            "toolcall_end" => {
                                if let Some(tool_call) = assistant_event.get("toolCall") {
                                    let name = tool_call
                                        .get("name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();
                                    let args =
                                        tool_call.get("arguments").cloned().unwrap_or(Value::Null);
                                    let args_str = serde_json::to_string(&args).unwrap_or_default();
                                    let _ = tx.send(AgentEvent::ToolUse(name, args_str));
                                }
                            }
                            "error" => {
                                let error_msg = assistant_event
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown error");
                                let _ =
                                    tx.send(AgentEvent::Error(format!("Pi error: {}", error_msg)));
                            }
                            _ => {
                                debug!("Unhandled Pi assistantMessageEvent type: {}", delta_type);
                            }
                        }
                    }
                }

                "tool_execution_start" => {
                    debug!(
                        "Pi tool start: {}",
                        msg.get("toolName").and_then(|v| v.as_str()).unwrap_or("?")
                    );
                }

                "tool_execution_end" => {
                    let tool_name = msg
                        .get("toolName")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let content = msg
                        .get("result")
                        .and_then(|r| r.get("content"))
                        .and_then(|c| c.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|block| block.get("text").and_then(|t| t.as_str()))
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default();
                    let is_error = msg
                        .get("isError")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let display_text = if !tool_name.is_empty() && !content.is_empty() {
                        format!("[{}]\n{}", tool_name, content)
                    } else {
                        content
                    };
                    let _ = tx.send(AgentEvent::ToolResult(display_text, is_error));
                }

                "extension_ui_request" => {
                    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
                    match method {
                        "confirm" => {
                            let id = msg
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let title = msg
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Confirmation")
                                .to_string();
                            let message = msg
                                .get("message")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            // Emit permission request first so the user knows what's being asked
                            let _ = tx.send(AgentEvent::PermissionRequest {
                                request_id: id.clone(),
                                tool_name: title.clone(),
                                input: Some(json!({"message": message})),
                            });

                            // Also emit confirm request for structured handling
                            if !id.is_empty() {
                                let prompt = if !message.is_empty() {
                                    format!("{}: {}", title, message)
                                } else {
                                    title
                                };
                                let _ = tx.send(AgentEvent::ConfirmRequest {
                                    request_id: id.clone(),
                                    prompt,
                                    options: vec!["Allow".to_string(), "Deny".to_string()],
                                });
                            }
                        }
                        "select" => {
                            let id = msg
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let title = msg
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Select")
                                .to_string();
                            let options: Vec<String> = msg
                                .get("options")
                                .and_then(|v| v.as_array())
                                .map(|arr| {
                                    arr.iter()
                                        .filter_map(|o| o.as_str().map(|s| s.to_string()))
                                        .collect()
                                })
                                .unwrap_or_default();

                            if !id.is_empty() && !options.is_empty() {
                                let _ = tx.send(AgentEvent::SelectRequest {
                                    request_id: id,
                                    prompt: title,
                                    options,
                                });
                            }
                        }
                        _ => {
                            debug!("Unhandled Pi extension UI method: {}", method);
                        }
                    }
                }

                "turn_start" | "turn_end" | "message_start" | "message_end" => {
                    // These provide structure but we handle streaming via message_update
                    debug!("Pi event: {}", event_type);
                }

                "compaction_start" | "compaction_end" | "auto_retry_start" | "auto_retry_end"
                | "queue_update" => {
                    debug!("Pi event (ignored): {}", event_type);
                }

                "extension_error" => {
                    let error_msg = msg
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("extension error");
                    let _ = tx.send(AgentEvent::Error(format!(
                        "Pi extension error: {}",
                        error_msg
                    )));
                }

                "" => {
                    if msg.get("id").is_some()
                        && (msg.get("result").is_some() || msg.get("error").is_some())
                    {
                        // JSON-RPC style response (shouldn't happen in RPC mode, but be safe)
                        debug!("Pi JSON-RPC style message: {}", line);
                    } else {
                        debug!("Unrecognized Pi event: {}", line);
                    }
                }

                other => {
                    debug!("Unknown Pi event type: {}", other);
                }
            }
        }

        // When stdout closes (EOF), ensure a Done event is sent so the
        // event_poller flushes any buffered text. Without this, the last
        // chunk — especially short sentences — can be silently dropped
        // if agent_end was never received or arrived before the last delta.
        let _ = tx.send(AgentEvent::Done);
        info!("Pi stdout reader ended");
        is_busy.store(false, Ordering::Relaxed);
    }

    async fn stderr_reader(
        stderr: tokio::process::ChildStderr,
        stderr_lines: Arc<StdMutex<Vec<String>>>,
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
                debug!("Pi stderr: {}", line);
            }
        }
    }
}
