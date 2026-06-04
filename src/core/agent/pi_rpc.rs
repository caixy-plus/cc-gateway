use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use crate::agent::event::AgentEvent;
use crate::agent::mcp_attach::prepare_pi_mcp;
use crate::config::model::{filter_pi_cli_tokens, strip_pi_cli_args, AgentConfig};
use crate::runtime::mcp_server::McpContext;

type PendingRpc = std::collections::HashMap<String, oneshot::Sender<Value>>;

/// Default RPC round-trip (prompt, abort, get_available_models, …).
const PI_RPC_TIMEOUT: Duration = Duration::from_secs(15);
/// Pi process may need a moment before stdin RPC accepts commands after spawn.
const PI_BOOTSTRAP_RPC_TIMEOUT: Duration = Duration::from_secs(10);
/// `switch_session` can load large JSONL histories; align with ACP session/load budget.
const PI_SESSION_SWITCH_TIMEOUT: Duration = Duration::from_secs(120);
const PI_READY_POLL_ATTEMPTS: u32 = 8;

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
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
        prepare_pi_mcp(&work_dir, mcp_context.as_ref()).await?;
        let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);

        let mut args = vec!["--mode".to_string(), "rpc".to_string()];

        // Profile default_args are normalized in config_for_provider; strip again so
        // `/agent pi --no-session` and stale configs cannot disable session persistence.
        if !config.default_args.is_empty() {
            for arg in strip_pi_cli_args(&config.default_args).split_whitespace() {
                args.push(arg.to_string());
            }
        }

        for arg in filter_pi_cli_tokens(&extra_args) {
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
        let pending_rpc: Arc<Mutex<PendingRpc>> =
            Arc::new(Mutex::new(std::collections::HashMap::new()));

        // Spawn stderr reader
        let stderr_lines = Arc::new(StdMutex::new(Vec::new()));
        tokio::spawn(Self::stderr_reader(stderr, stderr_lines.clone()));

        // Spawn stdout reader
        let tx = event_tx.clone();
        let busy = is_busy.clone();
        let stdin_clone = stdin.clone();
        let pending = pending_rpc.clone();
        tokio::spawn(Self::stdout_reader(stdout, tx, busy, stdin_clone, pending));

        let session = Self {
            child,
            stdin,
            is_busy,
            stderr_lines,
            pending_rpc,
        };

        session.wait_for_pi_rpc_ready().await?;

        if crate::command::agents::provider_supports_session_resume(&config.provider) {
            if let Some(ref session_path) = resume_session_id.filter(|s| !s.is_empty()) {
                session
                    .switch_session(session_path)
                    .await
                    .map_err(|e| pi_session_resume_error(session_path, &e.to_string()))?;
            }
        }

        let provider_session_id = session
            .session_file_from_state()
            .await
            .ok()
            .flatten();

        Ok((session, provider_session_id))
    }

    /// Poll until Pi accepts RPC commands (fresh `pi --mode rpc` can be slow to start).
    async fn wait_for_pi_rpc_ready(&self) -> Result<()> {
        for attempt in 1..=PI_READY_POLL_ATTEMPTS {
            match self
                .rpc_command_with_timeout(json!({"type": "get_state"}), PI_BOOTSTRAP_RPC_TIMEOUT)
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) if attempt < PI_READY_POLL_ATTEMPTS && rpc_error_is_timeout(&e) => {
                    debug!(
                        "Pi RPC not ready yet (attempt {}/{}): {}",
                        attempt, PI_READY_POLL_ATTEMPTS, e
                    );
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                Err(e) => return Err(e),
            }
        }
        Err(anyhow::anyhow!("Pi RPC did not become ready"))
    }

    /// Pi persists conversations to a JSONL file; `switch_session` reloads it after restart.
    pub async fn switch_session(&self, session_path: &str) -> Result<()> {
        let msg = self
            .rpc_command_with_timeout(
                json!({
                    "type": "switch_session",
                    "sessionPath": session_path,
                }),
                PI_SESSION_SWITCH_TIMEOUT,
            )
            .await?;
        if msg
            .get("data")
            .and_then(|d| d.get("cancelled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            anyhow::bail!(
                "{}",
                crate::t_fmt!("pi.session_resume_failed", PATH = session_path, ERR = "cancelled")
            );
        }
        Ok(())
    }

    async fn session_file_from_state(&self) -> Result<Option<String>> {
        let data = self.get_state().await?;
        Ok(extract_session_file(&data))
    }

    async fn get_state(&self) -> Result<Value> {
        let msg = self.rpc_command(json!({"type": "get_state"})).await?;
        msg.get("data")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Pi get_state response missing data"))
    }

    async fn rpc_command(&self, payload: Value) -> Result<Value> {
        self.rpc_command_with_timeout(payload, PI_RPC_TIMEOUT).await
    }

    async fn rpc_command_with_timeout(&self, mut payload: Value, timeout: Duration) -> Result<Value> {
        let id = uuid::Uuid::new_v4().to_string();
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("id".to_string(), json!(id));
        }
        let (tx, rx) = oneshot::channel();
        {
            let mut pending = self.pending_rpc.lock().await;
            pending.insert(id.clone(), tx);
        }
        self.write_json(&payload).await?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(msg)) => {
                if !msg.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                    let err = msg
                        .get("error")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Pi RPC command failed");
                    anyhow::bail!("{}", err);
                }
                Ok(msg)
            }
            Ok(Err(_)) => anyhow::bail!("Pi RPC response channel closed"),
            Err(_elapsed) => {
                // Remove the dead sender so stale entries don't accumulate in
                // pending_rpc and break the len==1 fallback for id-less responses.
                self.pending_rpc.lock().await.remove(&id);
                anyhow::bail!("Pi RPC command timed out after {}s", timeout.as_secs())
            }
        }
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
    /// Pi's `abort` is a control command with no documented response; fire-and-forget.
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

    /// Reset Pi context via `new_session` and return the new session file path.
    pub async fn new_provider_session(&self) -> Result<Option<String>> {
        let msg = self.rpc_command(json!({"type": "new_session"})).await?;
        if msg
            .get("data")
            .and_then(|d| d.get("cancelled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            anyhow::bail!("Pi new_session was cancelled");
        }
        self.session_file_from_state().await
    }

    pub async fn get_available_models(&self) -> Result<Vec<String>> {
        let msg = self
            .rpc_command(json!({"type": "get_available_models"}))
            .await?;
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
                    } else if msg.get("success").is_some() || msg.get("error").is_some() {
                        // Some Pi builds omit `id` on response; match sole pending caller.
                        let mut pending = pending_rpc.lock().await;
                        if pending.len() == 1 {
                            if let Some((_, tx)) = pending.drain().next() {
                                let _ = tx.send(msg.clone());
                            }
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
                            "done" => {
                                let reason = assistant_event
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("stop");
                                if reason != "toolUse" {
                                    // Text output for this turn is complete; flush before agent_end.
                                    let _ = tx.send(AgentEvent::Done);
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

fn pi_session_resume_error(session_path: &str, err: &str) -> anyhow::Error {
    let detail = if rpc_error_is_timeout_str(err) {
        crate::t!("pi.session_resume_timeout").to_string()
    } else {
        err.to_string()
    };
    anyhow::anyhow!(
        "{}",
        crate::t_fmt!("pi.session_resume_failed", PATH = session_path, ERR = detail)
    )
}

fn rpc_error_is_timeout(err: &anyhow::Error) -> bool {
    rpc_error_is_timeout_str(&err.to_string())
}

fn rpc_error_is_timeout_str(err: &str) -> bool {
    err.to_lowercase().contains("timed out")
}

fn extract_session_file(data: &Value) -> Option<String> {
    data.get("sessionFile")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pi_session_resume_error_maps_timeout_to_friendly_message() {
        let err = pi_session_resume_error("/tmp/s.jsonl", "Pi RPC command timed out after 15s");
        let msg = err.to_string();
        assert!(msg.contains("/tmp/s.jsonl"));
        assert!(!msg.contains("timed out after 15s"));
    }

    #[test]
    fn rpc_error_is_timeout_str_detects_timeout() {
        assert!(rpc_error_is_timeout_str("Pi RPC command timed out"));
        assert!(!rpc_error_is_timeout_str("session file missing"));
    }

    #[test]
    fn pi_session_resume_error_is_user_visible() {
        let err = pi_session_resume_error("/tmp/s.jsonl", "missing file");
        let msg = err.to_string();
        assert!(msg.contains("/tmp/s.jsonl"));
        assert!(msg.contains("missing file"));
    }

    #[test]
    fn extract_session_file_reads_session_file_field() {
        let data = json!({"sessionFile": "/tmp/s.jsonl", "sessionId": "abc"});
        assert_eq!(
            extract_session_file(&data).as_deref(),
            Some("/tmp/s.jsonl")
        );
        assert!(extract_session_file(&json!({})).is_none());
    }
}
