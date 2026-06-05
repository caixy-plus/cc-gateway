//! Shared ACP session semantics for Cursor, OpenCode, and future ACP providers.
//!
//! Provider-specific behavior lives in [`AcpHooks`]; this module owns spawn, prompt,
//! permission, session/update mapping, and lifecycle.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};

use crate::agent::acp_client::{
    emit_acp_turn_done, is_acp_turn_complete_update, reset_acp_turn_done,
    resolve_acp_spawn_session_id, AcpClient, NotificationHandler,
};
use crate::agent::event::AgentEvent;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// Context passed to provider-specific ACP notification handlers.
pub struct AcpNotifyCtx {
    pub event_tx: mpsc::UnboundedSender<AgentEvent>,
    pub pending_permissions: Arc<Mutex<HashMap<String, Value>>>,
    pub turn_done_sent: Arc<AtomicBool>,
    pub client_stdin: Arc<Mutex<ChildStdin>>,
}

/// Provider-specific hooks for an ACP agent CLI.
#[async_trait]
pub trait AcpHooks: Send + Sync + Copy + Default + 'static {
    fn log_provider_name(&self) -> &'static str;

    fn authenticate_method_id(&self) -> &str;

    fn default_permission_label(&self) -> &'static str;

    fn prompt_channel_closed_error(&self) -> &'static str;

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String;

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error;

    fn normalize_work_dir(work_dir: &str) -> Result<String> {
        Ok(work_dir.to_string())
    }

    async fn prepare_mcp_servers(
        work_dir: &str,
        mcp_context: Option<&McpContext>,
    ) -> Result<Value>;

    fn build_spawn_args(
        config: &AgentConfig,
        extra_args: Vec<String>,
        mcp_servers: &Value,
    ) -> Vec<String>;

    /// Handle non-standard ACP RPC notifications (e.g. Cursor extensions).
    /// Return `true` when the method was handled.
    fn handle_extension_notification(&self, method: &str, msg: &Value, ctx: &AcpNotifyCtx) -> bool;

    fn before_session_setup(
        &self,
        _event_tx: &mpsc::UnboundedSender<AgentEvent>,
        _config: &AgentConfig,
        _will_resume: bool,
    ) {
    }

    /// When true, [`GenericAcpSession`] implements in-session model switch via ACP.
    fn supports_acp_set_model(&self) -> bool {
        false
    }

    /// ACP `session/set_model` (and provider-specific fallbacks). Only used when
    /// [`Self::supports_acp_set_model`] is true.
    async fn set_session_model(
        &self,
        _session: &GenericAcpSession<Self>,
        _model_id: &str,
    ) -> Result<()> {
        anyhow::bail!("ACP set_session_model not implemented for this provider")
    }
}

pub struct GenericAcpSession<H: AcpHooks> {
    client: AcpClient,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
    turn_done_sent: Arc<AtomicBool>,
    mcp_servers: Value,
    pub(crate) hooks: H,
}

impl<H: AcpHooks> GenericAcpSession<H> {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
        let hooks = H::default();
        let work_dir = H::normalize_work_dir(&work_dir)?;
        let mcp_servers = H::prepare_mcp_servers(&work_dir, mcp_context.as_ref()).await?;
        let cli_path = crate::runtime::session::resolve_cli_path(&config.cli_path);
        let args = H::build_spawn_args(config, extra_args, &mcp_servers);

        info!(
            "Starting {} ACP session: {} {:?} in {}",
            hooks.log_provider_name(),
            cli_path,
            args,
            work_dir
        );

        let mut child = acp_spawn_command(&cli_path, &args)
            .current_dir(&work_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| H::spawn_failure_message(config, &cli_path))?;

        let stdin = child
            .stdin
            .take()
            .with_context(|| format!("Failed to open {} ACP stdin", hooks.log_provider_name()))?;
        let stdout = child
            .stdout
            .take()
            .with_context(|| format!("Failed to open {} ACP stdout", hooks.log_provider_name()))?;
        let stderr = child
            .stderr
            .take()
            .with_context(|| format!("Failed to open {} ACP stderr", hooks.log_provider_name()))?;

        let client = AcpClient::new(child, stdin);
        let pending = client.pending();
        let pending_permissions = client.pending_permissions();
        let client_stdin = client.stdin_arc();

        client.spawn_stderr_reader(stderr);

        let tx = event_tx.clone();
        let pp = pending_permissions.clone();
        let turn_done = Arc::new(AtomicBool::new(false));
        let turn_done_notify = turn_done.clone();
        let stdin_for_notify = client_stdin.clone();
        let hooks_for_notify = hooks;
        let on_notification: NotificationHandler = Arc::new(move |msg: &Value| {
            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let ctx = AcpNotifyCtx {
                event_tx: tx.clone(),
                pending_permissions: pp.clone(),
                turn_done_sent: turn_done_notify.clone(),
                client_stdin: stdin_for_notify.clone(),
            };
            match method {
                "session/update" => {
                    if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                        handle_session_update(update, &ctx.event_tx, &ctx.turn_done_sent);
                    }
                }
                "session/request_permission" => {
                    handle_session_request_permission(
                        msg,
                        &ctx,
                        hooks_for_notify.default_permission_label(),
                    );
                }
                other => {
                    if hooks_for_notify.handle_extension_notification(other, msg, &ctx) {
                        return;
                    }
                    if !other.is_empty() {
                        debug!(
                            "Unhandled {} ACP method: {}",
                            hooks_for_notify.log_provider_name(),
                            other
                        );
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
            hooks,
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
            .send_request(
                "authenticate",
                json!({ "methodId": session.hooks.authenticate_method_id() }),
            )
            .await?;

        let mode = acp_mode(config);
        let will_resume = resume_session_id.is_some();
        session
            .hooks
            .before_session_setup(&event_tx, config, will_resume);

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
                .map_err(|e| H::session_resume_error(sid, &e.to_string()))?;
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
        let closed_err = self.hooks.prompt_channel_closed_error().to_string();
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
                    let _ = event_tx.send(AgentEvent::Error(closed_err));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
            }
        });
        Ok(())
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
        let work_dir = H::normalize_work_dir(work_dir)?;
        let mode = acp_mode(config);
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
        let option_id = if allow { "once" } else { "reject" };
        self.client
            .write_json(json!({
                "jsonrpc": "2.0",
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

    pub(crate) fn acp_session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) async fn acp_request(&self, method: &str, params: Value) -> Result<Value> {
        self.send_request(method, params).await
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

/// Marker for sessions that use the shared ACP implementation without provider hooks.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoAcpHooks;

#[async_trait]
impl AcpHooks for NoAcpHooks {
    fn log_provider_name(&self) -> &'static str {
        "acp"
    }

    fn authenticate_method_id(&self) -> &str {
        ""
    }

    fn default_permission_label(&self) -> &'static str {
        "acp_permission"
    }

    fn prompt_channel_closed_error(&self) -> &'static str {
        "ACP prompt response channel closed"
    }

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn ACP agent. Is '{}' installed and on PATH? Tried '{}'.",
            config.cli_path, cli_path
        )
    }

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!("ACP session resume failed for {session_id}: {err}")
    }

    async fn prepare_mcp_servers(
        _work_dir: &str,
        _mcp_context: Option<&McpContext>,
    ) -> Result<Value> {
        Ok(json!([]))
    }

    fn build_spawn_args(
        config: &AgentConfig,
        extra_args: Vec<String>,
        _mcp_servers: &Value,
    ) -> Vec<String> {
        build_base_spawn_args(config, extra_args)
    }

    fn handle_extension_notification(&self, _method: &str, _msg: &Value, _ctx: &AcpNotifyCtx) -> bool {
        false
    }
}

pub fn acp_spawn_command(cli_path: &str, args: &[String]) -> Command {
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

pub fn build_base_spawn_args(config: &AgentConfig, extra_args: Vec<String>) -> Vec<String> {
    let mut args = Vec::new();
    if !config.default_args.is_empty() {
        args.extend(
            config
                .default_args
                .split_whitespace()
                .map(|s| s.to_string()),
        );
    }
    args.extend(extra_args);
    args
}

fn acp_mode(config: &AgentConfig) -> &str {
    if config.mode.trim().is_empty() {
        "agent"
    } else {
        config.mode.trim()
    }
}

pub fn rpc_id_key(id: &Value) -> String {
    id.as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| id.to_string())
}

pub fn extract_permission_label(params: &Value) -> Option<String> {
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

fn handle_session_request_permission(
    msg: &Value,
    ctx: &AcpNotifyCtx,
    default_label: &str,
) {
    let Some(id) = msg.get("id").cloned() else {
        return;
    };
    let key = rpc_id_key(&id);
    let pp = ctx.pending_permissions.clone();
    let key2 = key.clone();
    tokio::spawn(async move {
        pp.lock().await.insert(key2, id);
    });
    let params = msg.get("params").cloned();
    let tool_name = params
        .as_ref()
        .and_then(extract_permission_label)
        .unwrap_or_else(|| default_label.to_string());
    let _ = ctx.event_tx.send(AgentEvent::PermissionRequest {
        request_id: key,
        tool_name,
        input: params,
    });
}

pub fn handle_session_update(
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

/// Reply to a server-initiated ACP extension request (outside [`AcpClient::write_json`]).
pub fn respond_acp_extension(stdin: &Arc<Mutex<ChildStdin>>, msg: &Value, result: Value) {
    if let Some(id) = msg.get("id") {
        let stdin = stdin.clone();
        let result = result.clone();
        let id = id.clone();
        tokio::spawn(async move {
            let line = serde_json::to_string(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            }))
            .unwrap();
            let mut stdin = stdin.lock().await;
            let _ = stdin.write_all(line.as_bytes()).await;
            let _ = stdin.write_all(b"\n").await;
            let _ = stdin.flush().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::acp_client::extract_acp_session_id;

    #[test]
    fn extracts_acp_session_id_shapes() {
        assert_eq!(
            extract_acp_session_id(&json!({ "sessionId": "abc" })),
            Some("abc".to_string())
        );
        assert_eq!(
            extract_acp_session_id(&json!({ "session_id": "def" })),
            Some("def".to_string())
        );
    }

    #[test]
    fn maps_acp_text_update_to_agent_event() {
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
        assert!(!done.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn maps_acp_thinking_update_to_agent_event() {
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
    fn maps_acp_turn_complete_to_done() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let done = AtomicBool::new(false);
        handle_session_update(
            &json!({ "sessionUpdate": "agent_message_complete" }),
            &tx,
            &done,
        );
        assert!(matches!(rx.try_recv(), Ok(AgentEvent::Done)));
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
