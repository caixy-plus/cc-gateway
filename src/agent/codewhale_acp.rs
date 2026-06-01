use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::agent::acp_client::{
    emit_acp_turn_done, is_acp_turn_complete_update, reset_acp_turn_done, AcpClient,
};
use crate::agent::acp_fs::{read_for_acp_tag, try_handle_fs_request};
use crate::agent::codewhale_context::{
    build_history_transcript, build_prompt, fetch_capability, CodeWhaleCapability, ContextPolicy,
};
use crate::agent::event::AgentEvent;
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

pub struct CodeWhaleAcpSession {
    client: AcpClient,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
    turn_done_sent: Arc<AtomicBool>,
    work_dir: String,
    /// Gateway agent session id — keys `~/.cc-gateway/history/{id}.jsonl`.
    gateway_history_id: Option<String>,
    capability: CodeWhaleCapability,
    context_policy: ContextPolicy,
    mcp_servers: Value,
}

impl CodeWhaleAcpSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
        // Same validated absolute path the controller uses for spawn (ACP `cwd` must match).
        let work_dir = crate::runtime::controller::ensure_under_home(&work_dir)?;
        let work_dir_for_tags = work_dir.clone();

        let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);

        let mut args: Vec<String> = vec!["serve".to_string(), "--acp".to_string()];

        // Append default args from config
        if !config.default_args.is_empty() {
            for arg in config.default_args.split_whitespace() {
                args.push(arg.to_string());
            }
        }
        // Append extra args (passed via /agent codewhale <args>)
        for arg in extra_args {
            args.push(arg);
        }

        info!(
            "Starting CodeWhale ACP session: {} {:?} in {}",
            cli_path, args, work_dir
        );

        let mut cmd = Command::new(&cli_path);
        cmd.args(&args)
            .current_dir(&work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Pass through environment, filtering out provider-specific vars
        cmd.env_clear();
        for (k, v) in crate::agent::passthrough_env() {
            cmd.env(k, v);
        }

        let mut child = cmd.spawn().with_context(|| {
            format!(
                "Failed to spawn CodeWhale ACP. Is '{}' installed and on PATH? Tried '{} serve --acp'.",
                config.cli_path, cli_path
            )
        })?;

        let stdin = child
            .stdin
            .take()
            .context("Failed to open CodeWhale ACP stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("Failed to open CodeWhale ACP stdout")?;
        let stderr = child
            .stderr
            .take()
            .context("Failed to open CodeWhale ACP stderr")?;

        let client = AcpClient::new(child, stdin);
        let pending = client.pending();
        let pending_permissions = client.pending_permissions();

        client.spawn_stderr_reader(stderr);

        // Build notification handler
        let tx = event_tx.clone();
        let pp = pending_permissions.clone();
        let turn_done = Arc::new(AtomicBool::new(false));
        let turn_done_notify = turn_done.clone();
        let wd_tags = work_dir_for_tags;
        let wd_session = wd_tags.clone();
        let fs_stdin = client.stdin_arc();
        let on_notification: crate::agent::acp_client::NotificationHandler =
            Arc::new(move |msg: &Value| {
                if try_handle_fs_request(msg, &wd_tags, &fs_stdin) {
                    return;
                }
                let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
                match method {
                    "session/update" => {
                        if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                            handle_session_update(update, &tx, &turn_done_notify, &wd_tags);
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
                                .and_then(|p| p.get("toolCall"))
                                .and_then(|t| t.get("title").or_else(|| t.get("name")))
                                .and_then(|v| v.as_str())
                                .unwrap_or("codewhale_permission");
                            let _ = tx.send(AgentEvent::PermissionRequest {
                                request_id: key,
                                tool_name: tool_name.to_string(),
                                input: params,
                            });
                        }
                    }
                    other => {
                        if !other.is_empty() && !other.starts_with("codewhale/") {
                            debug!("Unhandled CodeWhale ACP method: {}", other);
                        }
                    }
                }
            });

        AcpClient::spawn_stdout_reader(stdout, pending, on_notification);

        let capability = fetch_capability(&config.cli_path).await;
        let gateway_history_id = resume_session_id.filter(|sid| !sid.trim().is_empty());
        let mcp_servers = build_acp_mcp_servers(mcp_context.as_ref())?;

        let session = Self {
            client,
            event_tx: event_tx.clone(),
            session_id: String::new(),
            turn_done_sent: turn_done,
            work_dir: wd_session,
            gateway_history_id,
            capability,
            context_policy: ContextPolicy::default(),
            mcp_servers,
        };

        // ACP handshake: initialize → session/new.
        // CodeWhale ACP advertises loadSession: false; session/load is never
        // supported, so we always create a new session and never persist the
        // ephemeral ACP session id as a resume token.
        session
            .send_request(
                "initialize",
                json!({
                    "protocolVersion": 1,
                    "clientCapabilities": {
                        "fs": { "readTextFile": true, "writeTextFile": true },
                        "terminal": false
                    },
                    "clientInfo": { "name": "cc-gateway", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await?;

        let mode = if config.mode.trim().is_empty() {
            "agent"
        } else {
            config.mode.trim()
        };

        let result = session
            .send_request(
                "session/new",
                json!({
                    "cwd": work_dir,
                    "mode": mode,
                    "mcpServers": session.mcp_servers,
                }),
            )
            .await?;

        let session_id = result
            .get("sessionId")
            .or_else(|| result.get("session_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("CodeWhale ACP did not return a session id"))?;

        let mut session = session;
        session.session_id = session_id;

        // ACP sessions are ephemeral — never persist as a resume token.
        Ok((session, None))
    }

    pub fn set_gateway_history_id(&mut self, id: String) {
        if !id.trim().is_empty() {
            self.gateway_history_id = Some(id);
        }
    }

    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        reset_acp_turn_done(&self.turn_done_sent);
        let history = self.gateway_history_id.as_deref().and_then(|sid| {
            build_history_transcript(
                sid,
                &self.capability,
                &self.context_policy,
                &self.work_dir,
                text,
            )
        });
        let work_dir_msg = build_prompt(&self.work_dir, history.as_deref(), text);
        let rx = self
            .send_request_detached(
                "session/prompt",
                json!({
                    "sessionId": self.session_id,
                    "prompt": [{ "type": "text", "text": work_dir_msg }]
                }),
            )
            .await?;
        let event_tx = self.event_tx.clone();
        let turn_done = self.turn_done_sent.clone();
        tokio::spawn(async move {
            match rx.await {
                Ok(Ok(_)) => {
                    tokio::time::sleep(std::time::Duration::from_millis(3000)).await;
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                Ok(Err(err)) => {
                    let _ = event_tx.send(AgentEvent::Error(err));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                Err(_) => {
                    let _ = event_tx.send(AgentEvent::Error(
                        "CodeWhale ACP prompt response channel closed".to_string(),
                    ));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
            }
        });
        Ok(())
    }

    pub async fn send_stop_generation(&mut self) -> Result<()> {
        self.client
            .write_json(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": self.session_id }
            }))
            .await
    }

    pub async fn send_control_response(&mut self, request_id: &str, allow: bool) -> Result<()> {
        let id_value = self
            .client
            .pending_permissions()
            .lock()
            .await
            .remove(request_id)
            .unwrap_or_else(|| Value::String(request_id.to_string()));
        let option_id = if allow { "allow-once" } else { "reject-once" };
        self.client
            .write_json(json!({
                "jsonrpc": "2.0",
                "id": id_value,
                "result": { "outcome": { "outcome": "selected", "optionId": option_id } }
            }))
            .await
    }

    /// Create a new ACP session in the same process and return its id.
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
                    "cwd": &work_dir,
                    "mode": mode,
                    "mcpServers": self.mcp_servers,
                }),
            )
            .await?;
        let session_id = result
            .get("sessionId")
            .or_else(|| result.get("session_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("CodeWhale ACP did not return a session id"))?;
        self.session_id = session_id;
        Ok(None)
    }

    pub async fn stop(self) -> Result<()> {
        let _ = self
            .client
            .write_json(json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": self.session_id }
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

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn rpc_id_key(id: &Value) -> String {
    id.as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.to_string())
}

fn resolve_acp_tags(text: &str, work_dir: &str) -> String {
    use regex::Regex;

    let re: Regex = Regex::new(
        r#"<acp:read_file\s+path="([^"]*)"(?:\s+offset="([^"]*)")?(?:\s+limit="([^"]*)")?\s*(?:/>|></acp:read_file>)"#,
    )
    .unwrap();

    let mut result = text.to_string();
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();

    for caps in re.captures_iter(text) {
        let full_match = caps.get(0).unwrap();
        let path = caps.get(1).map(|m| m.as_str()).unwrap_or("");
        let offset: usize = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(1);
        let limit: Option<usize> = caps.get(3).and_then(|m| m.as_str().parse().ok());

        let raw = read_for_acp_tag(work_dir, path, offset, limit);
        let content = if raw.starts_with('[') {
            raw
        } else {
            format!(
                "Content of {} (from offset {}):\n```\n{}\n```",
                path, offset, raw
            )
        };
        replacements.push((full_match.start(), full_match.end(), content));
    }

    for (start, end, content) in replacements.into_iter().rev() {
        result.replace_range(start..end, &content);
    }

    result
}

fn handle_session_update(
    update: &Value,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    turn_done_sent: &AtomicBool,
    work_dir: &str,
) {
    let kind = update
        .get("sessionUpdate")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if let Some(text) = update
        .get("content")
        .and_then(|c| c.get("text"))
        .and_then(|v| v.as_str())
    {
        let resolved = resolve_acp_tags(text, work_dir);
        let _ = event_tx.send(AgentEvent::Text(resolved));
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
