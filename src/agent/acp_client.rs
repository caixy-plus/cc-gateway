use anyhow::Result;
use serde_json::{json, Value};
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
                            Err(error.to_string())
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
        let _ = self.child.kill().await;
        info!("ACP session stopped");
        Ok(())
    }

    pub async fn force_stop(mut self) -> Result<()> {
        let _ = self.child.kill().await;
        info!("ACP session force-stopped");
        Ok(())
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
        let id = resolve_acp_spawn_session_id(
            &json!({ "configOptions": [] }),
            Some("ses_load_abc"),
        )
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
}
