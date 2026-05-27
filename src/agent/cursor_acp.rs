use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use crate::agent::event::{AgentEvent, QuestionItem, QuestionOption};
use crate::config::model::AgentConfig;

type PendingResponse = oneshot::Sender<std::result::Result<Value, String>>;

pub struct CursorAcpSession {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    next_id: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
    pending_permissions: Arc<Mutex<HashMap<String, Value>>>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
}

impl CursorAcpSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        resume_session_id: Option<String>,
    ) -> Result<(Self, Option<String>)> {
        let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);
        let mut args: Vec<String> = Vec::new();
        if !config.default_args.is_empty() {
            args.extend(
                config
                    .default_args
                    .split_whitespace()
                    .map(|s| s.to_string()),
            );
        }
        args.extend(extra_args);
        args.push("acp".to_string());

        info!(
            "Starting Cursor ACP session: {} {:?} in {}",
            cli_path, args, work_dir
        );

        let mut command = cursor_command(&cli_path, &args);
        let mut child = command
            .current_dir(&work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn Cursor Agent CLI. Is '{}' installed and on PATH? Tried '{}'.",
                    config.cli_path, cli_path
                )
            })?;

        let stdin = Arc::new(Mutex::new(
            child
                .stdin
                .take()
                .context("Failed to open Cursor ACP stdin")?,
        ));
        let stdout = child
            .stdout
            .take()
            .context("Failed to open Cursor ACP stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("Failed to open Cursor ACP stderr")?;

        let pending = Arc::new(Mutex::new(HashMap::<u64, PendingResponse>::new()));
        let pending_permissions = Arc::new(Mutex::new(HashMap::<String, Value>::new()));
        let next_id = Arc::new(AtomicU64::new(1));

        tokio::spawn(Self::stderr_reader(stderr));
        tokio::spawn(Self::stdout_reader(
            stdout,
            pending.clone(),
            pending_permissions.clone(),
            stdin.clone(),
            event_tx.clone(),
        ));

        let session = Self {
            child,
            stdin,
            next_id,
            pending,
            pending_permissions,
            event_tx: event_tx.clone(),
            session_id: String::new(),
        };

        session
            .send_request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": { "readTextFile": false, "writeTextFile": false },
                        "terminal": false
                    },
                    "clientInfo": { "name": "cc-gateway", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;

        session
            .send_request("authenticate", json!({ "methodId": "cursor_login" }))
            .await?;

        let mode = if config.mode.trim().is_empty() {
            "agent"
        } else {
            config.mode.trim()
        };
        let result = if let Some(ref sid) = resume_session_id {
            match session
                .send_request(
                    "session/load",
                    json!({
                        "sessionId": sid,
                        "cwd": work_dir,
                        "mode": mode,
                        "mcpServers": []
                    }),
                )
                .await
            {
                Ok(v) => v,
                Err(e) => {
                    // Cursor ACP session ids may not be resumable across agent process restarts.
                    // If the stored session id is no longer recognized, fall back to starting a new session.
                    let err = e.to_string();
                    if is_cursor_session_not_found_error(&err) {
                        let _ = event_tx.send(AgentEvent::Text(
                            "Cursor session not found; starting a new session.".to_string(),
                        ));
                        session
                            .send_request(
                                "session/new",
                                json!({
                                    "cwd": work_dir,
                                    "mode": mode,
                                    "mcpServers": []
                                }),
                            )
                            .await?
                    } else {
                        return Err(e);
                    }
                }
            }
        } else {
            session
                .send_request(
                    "session/new",
                    json!({
                        "cwd": work_dir,
                        "mode": mode,
                        "mcpServers": []
                    }),
                )
                .await?
        };

        let session_id = extract_session_id(&result)
            .ok_or_else(|| anyhow::anyhow!("Cursor ACP did not return a session id"))?;
        let mut session = session;
        session.session_id = session_id.clone();
        let _ = event_tx.send(AgentEvent::SessionId(session_id.clone()));

        Ok((session, Some(session_id)))
    }

    pub async fn send_user_message(&self, text: &str) -> Result<()> {
        let rx = self
            .send_request_detached(
                "session/prompt",
                json!({
                    "sessionId": self.session_id.clone(),
                    "prompt": [{ "type": "text", "text": text }]
                }),
            )
            .await?;
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            match rx.await {
                Ok(Ok(_)) => {
                    let _ = event_tx.send(AgentEvent::Done);
                }
                Ok(Err(err)) => {
                    let _ = event_tx.send(AgentEvent::Error(err));
                    let _ = event_tx.send(AgentEvent::Done);
                }
                Err(_) => {
                    let _ = event_tx.send(AgentEvent::Error(
                        "Cursor ACP prompt response channel closed".to_string(),
                    ));
                    let _ = event_tx.send(AgentEvent::Done);
                }
            }
        });
        Ok(())
    }

    pub async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()> {
        let id = self
            .pending_permissions
            .lock()
            .await
            .remove(request_id)
            .unwrap_or_else(|| Value::String(request_id.to_string()));
        let option_id = if allow { "allow-once" } else { "reject-once" };
        Self::write_json(
            &self.stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "outcome": { "outcome": "selected", "optionId": option_id } }
            }),
        )
        .await
    }

    pub async fn stop(mut self) -> Result<()> {
        let _ = Self::write_json(
            &self.stdin,
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": self.session_id.clone() }
            }),
        )
        .await;
        let _ = self.child.kill().await;
        info!("Cursor ACP session stopped");
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(None) => true,
            Ok(Some(status)) => {
                warn!("Cursor ACP process exited with status: {}", status);
                false
            }
            Err(e) => {
                warn!("Failed to check Cursor ACP process status: {}", e);
                false
            }
        }
    }

    async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        let rx = self.send_request_detached(method, params).await?;
        match rx.await.context("Cursor ACP response channel closed")? {
            Ok(value) => Ok(value),
            Err(err) => anyhow::bail!(err),
        }
    }

    async fn send_request_detached(
        &self,
        method: &str,
        params: Value,
    ) -> Result<oneshot::Receiver<std::result::Result<Value, String>>> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        Self::write_json(
            &self.stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }),
        )
        .await?;
        Ok(rx)
    }

    async fn write_json(stdin: &Arc<Mutex<ChildStdin>>, value: Value) -> Result<()> {
        let line = serde_json::to_string(&value)?;
        debug!("→ Cursor ACP: {}", line);
        let mut stdin = stdin.lock().await;
        stdin.write_all(line.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    async fn stdout_reader(
        stdout: tokio::process::ChildStdout,
        pending: Arc<Mutex<HashMap<u64, PendingResponse>>>,
        pending_permissions: Arc<Mutex<HashMap<String, Value>>>,
        stdin: Arc<Mutex<ChildStdin>>,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            debug!("← Cursor ACP: {}", line);

            let msg: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(e) => {
                    debug!("Non-JSON line from Cursor ACP: {} (err: {})", line, e);
                    continue;
                }
            };

            if let Some(id) = msg.get("id").and_then(|v| v.as_u64()) {
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

            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            match method {
                "session/update" => {
                    if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                        handle_session_update(update, &event_tx);
                    }
                }
                "session/request_permission" => {
                    if let Some(id) = msg.get("id").cloned() {
                        let key = rpc_id_key(&id);
                        pending_permissions.lock().await.insert(key.clone(), id);
                        let params = msg.get("params").cloned();
                        let tool_name = params
                            .as_ref()
                            .and_then(extract_permission_label)
                            .unwrap_or_else(|| "cursor_permission".to_string());
                        let _ = event_tx.send(AgentEvent::PermissionRequest {
                            request_id: key,
                            tool_name,
                            input: params,
                        });
                    }
                }
                "cursor/ask_question" => {
                    if let Some(params) = msg.get("params") {
                        if let Some(questions) = parse_cursor_questions(params) {
                            let request_id = msg
                                .get("id")
                                .map(rpc_id_key)
                                .unwrap_or_else(|| "cursor-question".to_string());
                            let _ = event_tx.send(AgentEvent::QuestionRequest {
                                request_id,
                                questions,
                            });
                        }
                    }
                    respond_extension(&stdin, &msg, json!({
                        "outcome": { "outcome": "skipped", "reason": "cc-gateway does not yet collect Cursor ACP question answers" }
                    }))
                    .await;
                }
                "cursor/create_plan" => {
                    if let Some(plan) = msg
                        .get("params")
                        .and_then(|p| p.get("plan"))
                        .and_then(|p| p.as_str())
                    {
                        let _ = event_tx
                            .send(AgentEvent::Text(format!("\n[Plan requested]\n{}\n", plan)));
                    }
                    respond_extension(&stdin, &msg, json!({
                        "outcome": { "outcome": "rejected", "reason": "Plan approval is not available through cc-gateway yet" }
                    }))
                    .await;
                }
                "cursor/update_todos" | "cursor/task" | "cursor/generate_image" => {
                    debug!("Cursor ACP extension notification: {}", method);
                }
                _ => {
                    if !method.is_empty() {
                        debug!("Unhandled Cursor ACP method: {}", method);
                    }
                }
            }
        }

        info!("Cursor ACP stdout reader ended");
    }

    async fn stderr_reader(stderr: tokio::process::ChildStderr) {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.trim().is_empty() {
                debug!("Cursor ACP stderr: {}", line);
            }
        }
    }
}

fn cursor_command(cli_path: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        let lower = cli_path.to_lowercase();
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(cli_path).args(args);
            return command;
        }
    }

    let mut command = Command::new(cli_path);
    command.args(args);
    command
}

fn extract_session_id(value: &Value) -> Option<String> {
    value
        .get("sessionId")
        .or_else(|| value.get("session_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn rpc_id_key(id: &Value) -> String {
    id.as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.to_string())
}

fn extract_permission_label(params: &Value) -> Option<String> {
    params
        .get("toolCall")
        .and_then(|v| v.get("title").or_else(|| v.get("name")))
        .and_then(|v| v.as_str())
        .or_else(|| {
            params
                .get("permission")
                .and_then(|v| v.get("name").or_else(|| v.get("title")))
                .and_then(|v| v.as_str())
        })
        .or_else(|| params.get("title").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

fn handle_session_update(update: &Value, event_tx: &mpsc::UnboundedSender<AgentEvent>) {
    if let Some(text) = update
        .get("content")
        .and_then(|c| c.get("text"))
        .and_then(|v| v.as_str())
    {
        let _ = event_tx.send(AgentEvent::Text(text.to_string()));
        return;
    }

    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if kind.contains("tool") {
        let _ = event_tx.send(AgentEvent::ToolUse(
            kind.to_string(),
            serde_json::to_string(update).unwrap_or_default(),
        ));
    } else if kind.contains("error") {
        let _ = event_tx.send(AgentEvent::Error(update.to_string()));
    }
}

fn parse_cursor_questions(params: &Value) -> Option<Vec<QuestionItem>> {
    let questions = params.get("questions")?.as_array()?;
    let parsed: Vec<QuestionItem> = questions
        .iter()
        .filter_map(|q| {
            let question = q.get("prompt")?.as_str()?.to_string();
            let options = q.get("options")?.as_array()?;
            let parsed_options: Vec<QuestionOption> = options
                .iter()
                .filter_map(|o| {
                    Some(QuestionOption {
                        label: o.get("label")?.as_str()?.to_string(),
                        description: o
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect();
            Some(QuestionItem {
                question,
                header: params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                options: parsed_options,
                multi_select: q
                    .get("allowMultiple")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
    }
}

async fn respond_extension(stdin: &Arc<Mutex<ChildStdin>>, msg: &Value, result: Value) {
    if let Some(id) = msg.get("id") {
        let _ = CursorAcpSession::write_json(
            stdin,
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            }),
        )
        .await;
    }
}

fn is_cursor_session_not_found_error(err: &str) -> bool {
    // Cursor ACP returns JSON-RPC errors as JSON strings, for example:
    // {"code":-32602,"data":{"message":"Session \"...\" not found"},"message":"Invalid params"}
    if !err.contains("Session") || !err.contains("not found") {
        return false;
    }
    // Best-effort JSON parse to avoid matching unrelated errors.
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(err) {
        let msg = v
            .get("data")
            .and_then(|d| d.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("");
        return msg.contains("Session") && msg.contains("not found");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_cursor_session_id_shapes() {
        assert_eq!(
            extract_session_id(&json!({ "sessionId": "abc" })),
            Some("abc".to_string())
        );
        assert_eq!(
            extract_session_id(&json!({ "session_id": "def" })),
            Some("def".to_string())
        );
    }

    #[test]
    fn parses_cursor_question_extension_payload() {
        let questions = parse_cursor_questions(&json!({
            "title": "Need input",
            "questions": [{
                "id": "q1",
                "prompt": "Which mode?",
                "allowMultiple": false,
                "options": [
                    { "id": "agent", "label": "Agent" },
                    { "id": "plan", "label": "Plan" }
                ]
            }]
        }))
        .expect("question should parse");

        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].header, "Need input");
        assert_eq!(questions[0].question, "Which mode?");
        assert_eq!(questions[0].options[0].label, "Agent");
    }

    #[test]
    fn maps_cursor_text_update_to_agent_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        handle_session_update(
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "text": "hello" }
            }),
            &tx,
        );

        match rx.try_recv().expect("event should be sent") {
            AgentEvent::Text(text) => assert_eq!(text, "hello"),
            other => panic!("expected text event, got {:?}", other),
        }
    }

    #[test]
    fn cursor_session_not_found_error_is_detected() {
        let err = r#"{"code":-32602,"data":{"message":"Session \"abc\" not found"},"message":"Invalid params"}"#;
        assert!(is_cursor_session_not_found_error(err));
        assert!(!is_cursor_session_not_found_error("other error"));
    }

    #[tokio::test]
    async fn real_cursor_acp_smoke_test_when_enabled() {
        if std::env::var("CC_GATEWAY_RUN_CURSOR_AGENT_TEST")
            .ok()
            .as_deref()
            != Some("1")
        {
            return;
        }

        let cli_path = std::env::var("CC_GATEWAY_CURSOR_AGENT_PATH")
            .unwrap_or_else(|_| r"C:\Users\volun\AppData\Local\cursor-agent\agent.cmd".to_string());
        let config = AgentConfig {
            provider: crate::config::model::AgentProvider::Cursor,
            cli_path,
            default_args: String::new(),
            mode: "agent".to_string(),
            permission: "prompt".to_string(),
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let work_dir = std::env::current_dir()
            .expect("current dir should be available")
            .to_string_lossy()
            .to_string();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            CursorAcpSession::spawn(work_dir, Vec::new(), &config, tx, None),
        )
        .await
        .expect("Cursor ACP smoke test timed out")
        .expect("Cursor ACP session should start");

        let (session, session_id) = result;
        assert!(session_id.as_deref().unwrap_or("").len() > 8);
        session.stop().await.expect("session should stop");
    }
}
