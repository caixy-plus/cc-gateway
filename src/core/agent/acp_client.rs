use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Parse JSON-RPC response `id` (string or numeric).
/// Cursor returns numeric ids (`1`).
fn parse_response_id(id: &Value) -> Option<u64> {
    if let Some(n) = id.as_u64() {
        return Some(n);
    }
    id.as_str()?.parse().ok()
}

pub type PendingResponse = oneshot::Sender<std::result::Result<Value, String>>;

/// Notification handler: receives the full JSON-RPC message (notification or
/// server-initiated request). The handler can extract method/params/id as needed.
pub type NotificationHandler = Arc<dyn Fn(&Value) + Send + Sync + 'static>;

/// Shared ACP JSON-RPC client over stdio.
pub struct AcpClient {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
    pending_permissions: Arc<Mutex<HashMap<String, Value>>>,
    stderr_lines: Arc<StdMutex<Vec<String>>>,
}

impl AcpClient {
    pub fn new(child: Child, stdin: ChildStdin) -> Self {
        Self {
            child,
            stdin: Arc::new(Mutex::new(stdin)),
            next_id: Arc::new(AtomicU64::new(1)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            pending_permissions: Arc::new(Mutex::new(HashMap::new())),
            stderr_lines: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    pub fn pending(&self) -> Arc<Mutex<HashMap<u64, PendingResponse>>> {
        self.pending.clone()
    }

    pub fn pending_permissions(&self) -> Arc<Mutex<HashMap<String, Value>>> {
        self.pending_permissions.clone()
    }

    pub fn stdin_arc(&self) -> Arc<Mutex<ChildStdin>> {
        self.stdin.clone()
    }

    // ------------------------------------------------------------------
    // JSON-RPC helpers
    // ------------------------------------------------------------------

    pub async fn write_json(&self, value: Value) -> Result<()> {
        let line = serde_json::to_string(&value)?;
        debug!("→ ACP: {}", line);
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let rx = self.send_request_detached(method, params).await?;
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(err))) => anyhow::bail!(err),
            Ok(Err(_)) => anyhow::bail!("ACP response channel closed"),
            Err(_) => anyhow::bail!(
                "ACP request timed out after {}s (method: {method})",
                REQUEST_TIMEOUT.as_secs()
            ),
        }
    }

    pub async fn send_request_detached(
        &self,
        method: &str,
        params: Value,
    ) -> Result<oneshot::Receiver<std::result::Result<Value, String>>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        self.write_json(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))
        .await?;
        Ok(rx)
    }

    // ------------------------------------------------------------------
    // spawn helpers
    // ------------------------------------------------------------------

    pub fn spawn_stdout_reader(
        stdout: ChildStdout,
        pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
        on_notification: NotificationHandler,
    ) {
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                debug!("← ACP: {}", line);

                let msg: Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(e) => {
                        debug!("Non-JSON line from ACP: {} (err: {})", line, e);
                        continue;
                    }
                };

                // Responses (id + result/error) → route to pending
                if let Some(id) = msg.get("id").and_then(parse_response_id) {
                    if msg.get("result").is_some() || msg.get("error").is_some() {
                        let result = if let Some(error) = msg.get("error") {
                            Err(format_rpc_error(error))
                        } else {
                            Ok(msg.get("result").cloned().unwrap_or(Value::Null))
                        };
                        if let Some(tx) = pending.lock().await.remove(&id) {
                            let _ = tx.send(result);
                        }
                        continue;
                    }
                }

                // Notifications + server-initiated requests → handler
                on_notification(&msg);
            }

            info!("ACP stdout reader ended");
        });
    }

    pub fn recent_stderr(&self) -> String {
        self.stderr_lines
            .lock()
            .map(|lines| lines.join("\n"))
            .unwrap_or_default()
    }

    pub fn stderr_buffer(&self) -> Arc<StdMutex<Vec<String>>> {
        self.stderr_lines.clone()
    }

    pub fn spawn_stderr_reader(&self, stderr: ChildStderr) {
        let stderr_lines = self.stderr_lines.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                if !line.trim().is_empty() {
                    if let Ok(mut buf) = stderr_lines.lock() {
                        buf.push(line.clone());
                        if buf.len() > 20 {
                            buf.remove(0);
                        }
                    }
                    debug!("ACP stderr: {}", line);
                }
            }
        });
    }

    // ------------------------------------------------------------------
    // lifecycle
    // ------------------------------------------------------------------

    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                warn!("ACP process exited with status: {}", status);
                false
            }
            Err(e) => {
                warn!("Failed to check ACP process status: {}", e);
                false
            }
        }
    }

    pub async fn stop(mut self) -> Result<()> {
        kill_child_process_tree(&mut self.child).await;
        info!("ACP session stopped");
        Ok(())
    }

    pub async fn force_stop(mut self) -> Result<()> {
        kill_child_process_tree(&mut self.child).await;
        info!("ACP session force-stopped");
        Ok(())
    }
}

/// Kill a child process and, on Windows, its entire process tree.
///
/// On Windows, `.cmd` / `.bat` wrappers are spawned via `cmd /C`, which
/// creates a two-level hierarchy (`cmd.exe` → real agent).  A plain
/// `child.kill()` only terminates `cmd.exe`, leaving the agent orphaned
/// with a visible console window.  `taskkill /T` terminates the whole tree.
pub(crate) async fn kill_child_process_tree(child: &mut Child) {
    let pid = child.id().unwrap_or(0);
    if pid == 0 {
        let _ = child.kill().await;
        return;
    }

    #[cfg(windows)]
    {
        let _ = tokio::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
    }

    // Still call kill() so the Child future is properly consumed / reaped
    // on all platforms.
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// Extract a user-visible message from a JSON-RPC `error` value.
pub fn format_rpc_error(error: &Value) -> String {
    if let Some(s) = error.as_str() {
        return s.to_string();
    }
    let message = error
        .get("message")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let data = error.get("data");
    let data_text = data.and_then(|d| {
        if let Some(s) = d.as_str() {
            (!s.is_empty()).then(|| s.to_string())
        } else if d.is_object() {
            d.get("message")
                .or_else(|| d.get("reason"))
                .or_else(|| d.get("error"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
        } else {
            None
        }
    });
    match (message, data_text) {
        (Some(msg), Some(extra)) if msg != extra => format!("{msg}: {extra}"),
        (Some(msg), _) => msg.to_string(),
        (None, Some(extra)) => extra,
        (None, None) => error.to_string(),
    }
}

/// Whether an ACP `session/update` marks the end of an assistant turn.
pub fn is_acp_turn_complete_update(kind: &str) -> bool {
    matches!(
        kind,
        "agent_message_complete"
            | "agent_message_done"
            | "turn_complete"
            | "agent_turn_complete"
            | "message_complete"
    ) || (kind.contains("complete") && !kind.contains("chunk"))
}

/// Emit at most one [`AgentEvent::Done`] per user prompt turn.
pub fn emit_acp_turn_done(
    event_tx: &tokio::sync::mpsc::UnboundedSender<crate::agent::event::AgentEvent>,
    done_sent: &std::sync::atomic::AtomicBool,
) {
    use std::sync::atomic::Ordering;
    if !done_sent.swap(true, Ordering::SeqCst) {
        let _ = event_tx.send(crate::agent::event::AgentEvent::Done);
    }
}

/// Reset the per-turn Done guard before sending a new prompt.
pub fn reset_acp_turn_done(done_sent: &std::sync::atomic::AtomicBool) {
    use std::sync::atomic::Ordering;
    done_sent.store(false, Ordering::SeqCst);
}

/// Reset the per-turn "had user-visible output" guard before a new prompt.
pub fn reset_acp_turn_content(had_content: &std::sync::atomic::AtomicBool) {
    use std::sync::atomic::Ordering;
    had_content.store(false, Ordering::SeqCst);
}

pub fn mark_acp_turn_content(had_content: Option<&std::sync::atomic::AtomicBool>) {
    use std::sync::atomic::Ordering;
    if let Some(flag) = had_content {
        flag.store(true, Ordering::SeqCst);
    }
}

/// Kimi (and some ACP agents) may return `session/prompt` success with `stopReason: end_turn`
/// but emit no `agent_message_chunk` when the provider API fails (e.g. inactive membership).
/// `fallback` is the provider-specific "no output" message
/// ([`crate::agent::acp_session::AcpHooks::no_output_message`]).
pub fn resolve_acp_empty_turn_message(result: &Value, stderr: &str, fallback: &str) -> String {
    if let Some(msg) = parse_provider_stderr_user_error(stderr) {
        return msg;
    }
    if let Some(stop) = result.get("stopReason").and_then(|v| v.as_str()) {
        if matches!(stop, "refusal" | "cancelled" | "max_tokens" | "max_turn_requests")
            || stop.contains("error")
        {
            return stop.to_string();
        }
    }
    fallback.to_string()
}

fn parse_provider_stderr_user_error(stderr: &str) -> Option<String> {
    for line in stderr.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let payload = trimmed
            .strip_prefix("error:")
            .map(str::trim)
            .unwrap_or(trimmed);
        if payload.contains("provider.api_error")
            || payload.contains("membership")
            || payload.contains("402")
        {
            return Some(friendly_provider_api_error(payload));
        }
    }
    None
}

fn friendly_provider_api_error(raw: &str) -> String {
    if raw.contains("402")
        || raw.contains("membership")
        || raw.contains("subscription")
        || raw.contains("套餐")
        || raw.contains("订阅")
    {
        return crate::t!("agent.kimi_subscription_required").to_string();
    }
    raw
        .split("provider.api_error:")
        .nth(1)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(raw)
        .to_string()
}

pub fn extract_acp_session_id(value: &Value) -> Option<String> {
    value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Some ACP agents omit `sessionId` in `session/load` results; reuse the load request id.
pub fn resolve_acp_spawn_session_id(
    result: &Value,
    loaded_session_id: Option<&str>,
) -> Result<String> {
    if let Some(id) = extract_acp_session_id(result) {
        return Ok(id);
    }
    if let Some(sid) = loaded_session_id.map(str::trim).filter(|s| !s.is_empty()) {
        return Ok(sid.to_string());
    }
    anyhow::bail!("ACP did not return a session id")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_acp_spawn_session_id_uses_loaded_id_when_load_omits_session_id() {
        let id =
            resolve_acp_spawn_session_id(&json!({ "configOptions": [] }), Some("ses_load_abc"))
                .expect("load responses without sessionId should reuse request id");
        assert_eq!(id, "ses_load_abc");
    }

    #[test]
    fn parse_response_id_accepts_numeric_and_string_ids() {
        assert_eq!(parse_response_id(&json!(1)), Some(1));
        assert_eq!(parse_response_id(&json!("2")), Some(2));
        assert_eq!(parse_response_id(&json!("abc")), None);
        assert_eq!(parse_response_id(&json!(null)), None);
    }

    #[test]
    fn acp_turn_complete_kinds() {
        assert!(is_acp_turn_complete_update("agent_message_complete"));
        assert!(is_acp_turn_complete_update("turn_complete"));
        assert!(!is_acp_turn_complete_update("agent_message_chunk"));
    }

    #[test]
    fn resolve_acp_empty_turn_message_maps_membership_stderr() {
        let stderr = "error: failed to run prompt: provider.api_error: 402 membership inactive";
        let msg =
            resolve_acp_empty_turn_message(&json!({"stopReason": "end_turn"}), stderr, "fallback");
        assert_ne!(msg, "end_turn");
        assert_ne!(msg, "fallback");
    }

    #[test]
    fn resolve_acp_empty_turn_message_uses_provider_fallback_when_silent_end_turn() {
        let msg = resolve_acp_empty_turn_message(
            &json!({"stopReason": "end_turn"}),
            "",
            "Gemini returned no output",
        );
        assert_eq!(msg, "Gemini returned no output");
    }

    #[test]
    fn format_rpc_error_extracts_message_and_data() {
        assert_eq!(
            format_rpc_error(&json!({
                "code": -32000,
                "message": "authRequired",
                "data": { "reason": "login required" }
            })),
            "authRequired: login required"
        );
        assert_eq!(
            format_rpc_error(&json!("subscription expired")),
            "subscription expired"
        );
    }
}
