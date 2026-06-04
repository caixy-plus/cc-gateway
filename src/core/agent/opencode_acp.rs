use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::debug;

use crate::agent::acp_client::{
    emit_acp_turn_done, is_acp_turn_complete_update, reset_acp_turn_done,
    resolve_acp_spawn_session_id, AcpClient,
};
use crate::agent::event::AgentEvent;
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// OpenCode ACP client (`opencode acp` — NDJSON JSON-RPC over stdio).
pub struct OpenCodeAcpSession {
    client: AcpClient,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
    turn_done_sent: Arc<AtomicBool>,
    mcp_servers: Value,
}

impl OpenCodeAcpSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
        let work_dir = crate::runtime::controller::ensure_under_home(&work_dir)?;
        let mcp_servers = build_acp_mcp_servers(mcp_context.as_ref())?;
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

        tracing::info!(
            "Starting OpenCode ACP session: {} {:?} in {}",
            cli_path,
            args,
            work_dir
        );

        let mut child = opencode_command(&cli_path, &args)
            .current_dir(&work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| {
                format!(
                    "Failed to spawn OpenCode ACP. Is '{}' installed and on PATH? Tried '{} acp'.",
                    config.cli_path, cli_path
                )
            })?;

        let stdin = child
            .stdin
            .take()
            .context("Failed to open OpenCode ACP stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("Failed to open OpenCode ACP stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("Failed to open OpenCode ACP stderr")?;

        let client = AcpClient::new(child, stdin);
        let pending = client.pending();
        let pending_permissions = client.pending_permissions();

        client.spawn_stderr_reader(stderr);

        let tx = event_tx.clone();
        let pp = pending_permissions.clone();
        let turn_done = Arc::new(AtomicBool::new(false));
        let turn_done_notify = turn_done.clone();
        let on_notification: crate::agent::acp_client::NotificationHandler =
            Arc::new(move |msg: &Value| {
                let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
                match method {
                    "session/update" => {
                        if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                            handle_session_update(update, &tx, &turn_done_notify);
                        }
                    }
                    "session/request_permission" => {
                        if let Some(id) = msg.get("id").cloned() {
                            let key = rpc_id_key(&id);
                            let pp2 = pp.clone();
                            let key2 = key.clone();
                            tokio::spawn(async move {
                                pp2.lock().await.insert(key2, id);
                            });
                            let params = msg.get("params").cloned();
                            let tool_name = params
                                .as_ref()
                                .and_then(extract_permission_label)
                                .unwrap_or_else(|| "opencode_permission".to_string());
                            let _ = tx.send(AgentEvent::PermissionRequest {
                                request_id: key,
                                tool_name,
                                input: params,
                            });
                        }
                    }
                    other => {
                        if !other.is_empty() {
                            debug!("Unhandled OpenCode ACP method: {}", other);
                        }
                    }
                }
            });

        AcpClient::spawn_stdout_reader(stdout, pending, on_notification);

        let session = Self {
            client,
            event_tx: event_tx.clone(),
            session_id: String::new(),
            turn_done_sent: turn_done,
            mcp_servers,
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
            .send_request("authenticate", json!({ "methodId": "opencode-login" }))
            .await?;

        let mode = if config.mode.trim().is_empty() {
            "agent"
        } else {
            config.mode.trim()
        };

        let (result, loaded_session_id) = if let Some(ref sid) = resume_session_id {
            let v = session
                .send_request(
                    "session/load",
                    json!({
                        "sessionId": sid,
                        "cwd": work_dir,
                        "mode": mode,
                        "mcpServers": session.mcp_servers
                    }),
                )
                .await
                .map_err(|e| opencode_session_resume_error(sid, &e.to_string()))?;
            (v, Some(sid.clone()))
        } else {
            let v = session
                .send_request(
                    "session/new",
                    json!({
                        "cwd": work_dir,
                        "mode": mode,
                        "mcpServers": session.mcp_servers
                    }),
                )
                .await?;
            (v, None)
        };

        let session_id = resolve_acp_spawn_session_id(&result, loaded_session_id.as_deref())?;
        let mut session = session;
        session.session_id = session_id.clone();
        let _ = event_tx.send(AgentEvent::SessionId(session_id.clone()));

        Ok((session, Some(session_id)))
    }

    pub async fn send_user_message(&self, text: &str) -> Result<()> {
        reset_acp_turn_done(&self.turn_done_sent);
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
        let turn_done = self.turn_done_sent.clone();
        tokio::spawn(async move {
            match rx.await {
                Ok(Ok(_)) => {
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                Ok(Err(err)) => {
                    let _ = event_tx.send(AgentEvent::Error(err));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                Err(_) => {
                    let _ = event_tx.send(AgentEvent::Error(
                        "OpenCode ACP prompt response channel closed".to_string(),
                    ));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
            }
        });
        Ok(())
    }

    pub async fn set_model(&self, model_id: &str) -> Result<()> {
        let params = json!({
            "sessionId": self.session_id,
            "modelId": model_id
        });
        match self.send_request("session/set_model", params.clone()).await {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("Method not found") => {
                // Older OpenCode builds exposed this as unstable_setSessionModel.
                self.send_request("unstable_setSessionModel", params)
                    .await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    pub async fn send_cancel(&self) -> Result<()> {
        self.client
            .write_json(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": self.session_id.clone() }
            }))
            .await
    }

    pub async fn new_provider_session(
        &mut self,
        work_dir: &str,
        config: &AgentConfig,
    ) -> Result<Option<String>> {
        let work_dir = crate::runtime::controller::ensure_under_home(work_dir)?;
        let mode = if config.mode.trim().is_empty() {
            "agent"
        } else {
            config.mode.trim()
        };
        let result = self
            .send_request(
                "session/new",
                json!({
                    "cwd": work_dir,
                    "mode": mode,
                    "mcpServers": self.mcp_servers,
                }),
            )
            .await?;
        let session_id = resolve_acp_spawn_session_id(&result, None)?;
        self.session_id = session_id.clone();
        let _ = self
            .event_tx
            .send(AgentEvent::SessionId(session_id.clone()));
        Ok(Some(session_id))
    }

    pub async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()> {
        let id_value = self
            .client
            .pending_permissions()
            .lock()
            .await
            .remove(request_id)
            .unwrap_or_else(|| Value::String(request_id.to_string()));
        // OpenCode ACP permission options use stable ids: `once` / `always` / `reject`.
        // We only support the one-shot decision here.
        let option_id = if allow { "once" } else { "reject" };
        self.client
            .write_json(json!({
                "jsonrpc": "2.0",
                // JSON-RPC ids can be string or number; echo it back as-is.
                "id": id_value,
                "result": { "outcome": { "outcome": "selected", "optionId": option_id } }
            }))
            .await
    }

    pub async fn stop(self) -> Result<()> {
        let _ = self
            .client
            .write_json(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": self.session_id.clone() }
            }))
            .await;
        self.client.stop().await
    }

    pub async fn force_stop(self) -> Result<()> {
        self.client.force_stop().await
    }

    pub fn is_alive(&mut self) -> bool {
        self.client.is_alive()
    }

    pub fn recent_stderr(&self) -> String {
        self.client.recent_stderr()
    }

    async fn send_request(&self, method: &str, params: Value) -> Result<Value> {
        self.client.send_request(method, params).await
    }

    async fn send_request_detached(
        &self,
        method: &str,
        params: Value,
    ) -> Result<tokio::sync::oneshot::Receiver<std::result::Result<Value, String>>> {
        self.client.send_request_detached(method, params).await
    }
}

fn opencode_command(cli_path: &str, args: &[String]) -> Command {
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

fn rpc_id_key(id: &Value) -> String {
    id.as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.to_string())
}

fn extract_permission_label(params: &Value) -> Option<String> {
    // Prefer stable tool identifiers (so we can auto-approve MCP send_file),
    // then fall back to human-facing labels.
    params
        .get("toolCall")
        .and_then(|v| {
            v.get("name")
                .or_else(|| v.get("toolName"))
                .or_else(|| v.get("tool_name"))
                .or_else(|| v.get("id"))
        })
        .and_then(|v| v.as_str())
        .or_else(|| {
            params
                .get("permission")
                .and_then(|v| {
                    v.get("name")
                        .or_else(|| v.get("toolName"))
                        .or_else(|| v.get("tool_name"))
                        .or_else(|| v.get("id"))
                })
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            params
                .get("toolCall")
                .and_then(|v| v.get("title"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            params
                .get("permission")
                .and_then(|v| v.get("title"))
                .and_then(|v| v.as_str())
        })
        .or_else(|| params.get("title").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

fn handle_session_update(
    update: &Value,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    turn_done_sent: &AtomicBool,
) {
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if let Some(thinking) = update
        .get("content")
        .and_then(|c| {
            c.get("thinking")
                .or_else(|| c.get("reasoning"))
                .or_else(|| c.get("thought"))
        })
        .and_then(|v| v.as_str())
    {
        let _ = event_tx.send(AgentEvent::Thinking(thinking.to_string()));
        if is_acp_turn_complete_update(kind) {
            emit_acp_turn_done(event_tx, turn_done_sent);
        }
        return;
    }

    if let Some(text) = update
        .get("content")
        .and_then(|c| c.get("text"))
        .and_then(|v| v.as_str())
    {
        let _ = event_tx.send(AgentEvent::Text(text.to_string()));
        if is_acp_turn_complete_update(kind) {
            emit_acp_turn_done(event_tx, turn_done_sent);
        }
        return;
    }

    if kind.contains("tool") {
        let _ = event_tx.send(AgentEvent::ToolUse(
            kind.to_string(),
            serde_json::to_string(update).unwrap_or_default(),
        ));
    } else if kind.contains("error") {
        let _ = event_tx.send(AgentEvent::Error(update.to_string()));
    } else if is_acp_turn_complete_update(kind) {
        emit_acp_turn_done(event_tx, turn_done_sent);
    }
}

fn opencode_session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        crate::t_fmt!("opencode.session_resume_failed", ID = session_id, ERR = err)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_acp_spawn_session_id_prefers_result_session_id() {
        let id = resolve_acp_spawn_session_id(
            &json!({ "sessionId": "from-result" }),
            Some("from-load-request"),
        )
        .expect("should resolve");
        assert_eq!(id, "from-result");
    }

    #[test]
    fn maps_opencode_text_update_to_agent_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let done = AtomicBool::new(false);
        handle_session_update(
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "text": "hello" }
            }),
            &tx,
            &done,
        );

        match rx.try_recv().expect("event should be sent") {
            AgentEvent::Text(text) => assert_eq!(text, "hello"),
            other => panic!("expected text event, got {:?}", other),
        }
    }

    #[test]
    fn maps_opencode_thinking_update_to_agent_event() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let done = AtomicBool::new(false);
        handle_session_update(
            &json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "thinking": "hmm" }
            }),
            &tx,
            &done,
        );

        match rx.try_recv().expect("event should be sent") {
            AgentEvent::Thinking(text) => assert_eq!(text, "hmm"),
            other => panic!("expected thinking event, got {:?}", other),
        }
    }

    #[test]
    fn opencode_session_resume_error_is_user_visible() {
        let err = opencode_session_resume_error("sess-1", "timeout");
        let msg = err.to_string();
        assert!(msg.contains("sess-1"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn extracts_permission_tool_name_prefers_toolcall_name() {
        let params = json!({
            "toolCall": { "name": "mcp__cc-gateway__send_file", "title": "Send file" }
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("mcp__cc-gateway__send_file".to_string())
        );
    }

    #[test]
    fn extracts_permission_tool_name_falls_back_to_title() {
        let params = json!({
            "toolCall": { "title": "cc-gateway send_file" }
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("cc-gateway send_file".to_string())
        );
    }
}
