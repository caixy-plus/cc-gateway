//! Shared **ACP** (Agent Communication Protocol) session implementation across providers.
//!
//! # Protocol
//!
//! ACP is a protocol based on stdio NDJSON JSON-RPC, with standard methods including:
//!
//! - `initialize` → Handshake;
//! - `session/new` → Start a new session, optionally carrying `mcpServers`;
//! - `session/load` → Restore a session using a persisted `provider_session_id`;
//! - `session/prompt` → Send user messages;
//! - `session/cancel` → Abort generation;
//! - `session/set_model` / `session/set_config_option` → Switch model;
//! - `authenticate` → Authentication (skipped by some providers that use cached credentials).
//!
//! For details, see [`https://agentclientprotocol.com`](https://agentclientprotocol.com).
//!
//! # Design
//!
//! [`GenericAcpSession`] holds an [`AcpClient`] (stdio JSON-RPC transport) and provider-specific
//! [`AcpHooks`]. Codex, Cursor, OpenCode, Kimi, Gemini, and **Qoder** are all integrated
//! via a "thin hook layer" without needing to rewrite transport, spawn, or prompt logic.
//!
//! # Key Types
//!
//! - [`AcpHooks`]: Defines provider differences (argv, authentication, MCP, model switching, extension notifications).
//! - [`GenericAcpSession`]: The main session runner implementing [`super::backend::AgentBackend`].
//! - [`AcpNotifyCtx`]: Context required for non-standard ACP notification callbacks.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info};

use crate::agent::acp_client::{
    emit_acp_turn_done, is_acp_turn_complete_update, mark_acp_turn_content, reset_acp_turn_content,
    reset_acp_turn_done, resolve_acp_empty_turn_message, resolve_acp_spawn_session_id, AcpClient,
    NotificationHandler,
};
use crate::agent::event::AgentEvent;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// How long to wait before concluding that the session is hung and actively ending the turn,
/// when the agent exhibits **no** ACP traffic during a `session/prompt` request.
///
/// Note: The response to `session/prompt` can take several minutes (as it waits for the turn to complete before returning),
/// so the watchdog monitors "inbound activity" rather than "total duration". The provider might not emit status updates
/// for a long time during tool execution — this timeout must be large enough to only catch actual hangs
/// (e.g., when a Kimi membership has expired).
const PROMPT_IDLE_TIMEOUT: Duration = Duration::from_secs(600);
const PROMPT_IDLE_TICK: Duration = Duration::from_secs(10);

/// Context passed to custom ACP notification callbacks of the provider.
///
/// Contains the event sender, pending permission request maps, atomic indicators for turn completion and presence of content,
/// and the child process stdin (in case callbacks need to send subsequent requests to the provider).
pub struct AcpNotifyCtx {
    pub event_tx: mpsc::UnboundedSender<AgentEvent>,
    pub pending_permissions: Arc<Mutex<HashMap<String, Value>>>,
    /// ACP `params.options` arrays keyed by request id; consumed by
    /// [`GenericAcpSession::send_permission_response`] to pick the real `optionId`.
    pub pending_permission_options: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    pub turn_done_sent: Arc<AtomicBool>,
    pub turn_had_content: Arc<AtomicBool>,
    pub client_stdin: Arc<Mutex<ChildStdin>>,
}

/// Provider-specific hooks: Connects the generic ACP flow of [`GenericAcpSession`] with concrete provider details
/// such as argv, authentication, and model switching.
///
/// Implementers only need to describe "how this provider differs from standard ACP"; all other spawn, prompt,
/// permission, and session update mappings are provided by [`GenericAcpSession`].
#[async_trait]
pub trait AcpHooks: Send + Sync + Copy + Default + 'static {
    /// The provider name used in logs and user-visible errors (e.g., `"Qoder"` / `"Gemini"`).
    fn log_provider_name(&self) -> &'static str;

    /// The method ID for the ACP `authenticate` call. Returns `None` to **skip** the `authenticate` RPC
    /// (applicable for providers using cached CLI credentials or PAT environment variables, e.g., Gemini, Codex, **Qoder**).
    fn authenticate_method_id(&self) -> Option<&str>;

    /// The default label displayed for permission request messages in the WebUI / bot (e.g., `"qoder_permission"`).
    fn default_permission_label(&self) -> &'static str;

    /// The error message when the prompt channel is closed (the child process exited abnormally).
    fn prompt_channel_closed_error(&self) -> &'static str;

    /// User hint when spawn fails, including `config.cli_path` and the actual cli path resolved,
    /// making it easier to troubleshoot cases where the user has overridden `cli_path` in `config.json`.
    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String;

    /// The error returned when session restoration fails; should use i18n to support bilingual messages.
    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error;

    /// Normalizes the user's `work_dir` (typical use case: restricting under `$HOME` to prevent
    /// the provider from triggering permission errors when writing session files). Defaults to direct passthrough.
    fn normalize_work_dir(work_dir: &str) -> Result<String> {
        Ok(work_dir.to_string())
    }

    /// Converts the gateway MCP context (capabilities like `send_file`) into the ACP standard `mcpServers`
    /// JSON field; returns an empty object `{}` if no servers are attached.
    async fn prepare_mcp_servers(work_dir: &str, mcp_context: Option<&McpContext>)
        -> Result<Value>;

    /// Assembles the final argv of the provider CLI:
    ///
    /// ```text
    /// <default_args> <extra_args> <provider-specific subcommand / flag>
    /// ```
    ///
    /// It is recommended to use [`build_base_spawn_args`] to perform a unified normalization of
    /// "registry defaults + profile overrides + stripping cross-provider flags" before appending provider-specific tokens.
    fn build_spawn_args(
        config: &AgentConfig,
        extra_args: Vec<String>,
        mcp_servers: &Value,
    ) -> Vec<String>;

    /// Processes non-standard ACP notifications (e.g., Cursor extension notifications).
    ///
    /// Returns `true` to indicate this method handled it, and [`GenericAcpSession`] will skip the default branch;
    /// returns `false` to hand it over to the default processing branch.
    fn handle_extension_notification(&self, method: &str, msg: &Value, ctx: &AcpNotifyCtx) -> bool;

    /// Hook executed before the ACP session ID is established (a few providers require injecting custom behavior here).
    fn before_session_setup(
        &self,
        _event_tx: &mpsc::UnboundedSender<AgentEvent>,
        _config: &AgentConfig,
        _will_resume: bool,
    ) {
    }

    /// Hook executed after the ACP session ID is established.
    ///
    /// Typical use case: Codex ignores the `mode` parameter in `session/new` and needs it issued separately
    /// via `session/set_mode` after spawning is complete. Failures in `after_session_setup` **should not**
    /// cause the overall spawn operation to fail (simply log it with `tracing::warn!`).
    async fn after_session_setup(&self, _session: &GenericAcpSession<Self>, _config: &AgentConfig) {
    }

    /// Whether in-session model switching is supported via ACP. When returning `true`, [`set_session_model`]
    /// will be called when the user runs the `/models` command.
    fn supports_acp_set_model(&self) -> bool {
        false
    }

    /// The message shown to users when `session/prompt` returns successfully but **no user-visible output**
    /// is generated during the entire turn.
    ///
    /// Defaults to using the `agent.acp_no_response` key (replacing the provider name using `log_provider_name()`).
    fn no_output_message(&self) -> String {
        crate::t_fmt!("agent.acp_no_response", NAME = self.log_provider_name())
    }

    /// ACP `session/set_model` (along with provider-specific fallback).
    ///
    /// Invoked only when [`Self::supports_acp_set_model`] returns `true`.
    /// The default implementation directly bails; concrete providers should override it (typical pattern: try `session/set_model` first,
    /// and downgrade to `session/set_config_option` if it fails with `Method not found`).
    async fn set_session_model(
        &self,
        _session: &GenericAcpSession<Self>,
        _model_id: &str,
    ) -> Result<()> {
        anyhow::bail!("ACP set_session_model not implemented for this provider")
    }
}

/// Generic session implementation for ACP providers.
///
/// Injects provider-specific details (argv, authentication, MCP, model switching) via [`AcpHooks`],
/// while handling stdio transport, JSON-RPC lifecycle, permission mapping, turn watchdog,
/// event translation, and all other common logic.
pub struct GenericAcpSession<H: AcpHooks> {
    /// Stdio JSON-RPC client (holding child stdin/stdout).
    client: AcpClient,
    /// Channel used to push [`AgentEvent`]s to [`crate::core::runtime::controller::AgentController`].
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    /// ACP-side session ID (returned by `session/new` or `session/load`).
    session_id: String,
    /// Whether the `Done` event has already been sent for the current turn (to prevent duplicate transmissions).
    turn_done_sent: Arc<AtomicBool>,
    /// Whether user-visible content was received in the current turn (used to identify "empty turns").
    turn_had_content: Arc<AtomicBool>,
    /// Timestamp of the most recent inbound ACP notification, driving the prompt watchdog.
    last_activity: Arc<StdMutex<Instant>>,
    /// The MCP servers JSON field passed to the provider during `session/new` / `session/load`,
    /// cached for subsequent `session/cancel` / restart paths.
    mcp_servers: Value,
    /// List of available models returned by `session/new` / `session/load` (from `configOptions`
    /// or Gemini's `models` field).
    available_models: Vec<String>,
    /// Currently active model ID (communicated by some providers during the session setup stage).
    active_model: Option<String>,
    /// Provider-specific hooks.
    pub(crate) hooks: H,
    /// Cache of ACP `params.options` per `session/request_permission` request id.
    ///
    /// The ACP spec defines `options` as an array of `{ optionId, name, kind }` where
    /// `optionId` is **agent-defined** (not one of the fixed values `"once"` / `"reject"`).
    /// We store the array here when a request arrives, then look up the matching option by
    /// its `kind` (e.g. `allow_once` / `reject_once`) when the user responds.
    pending_permission_options: Arc<Mutex<HashMap<String, Vec<Value>>>>,
}

/// Pick an ACP `optionId` from an `options` array by the user's intent (`allow` / `deny`).
///
/// Per the ACP v1 spec, each option carries a `kind` enum (`allow_once` / `allow_always`
/// / `reject_once` / `reject_always`) plus an **agent-defined** `optionId` string. The
/// gateway must match by `kind` and return the provider's own `optionId` verbatim.
///
/// Preference order:
///   - allow → `allow_once` first, then `allow_always` (least-privilege)
///   - deny  → `reject_once` first, then `reject_always`
///
/// Returns `None` when no option matches any of the wanted kinds — caller should
/// decide on a defensive fallback.
fn pick_option_id(options: &[Value], allow: bool) -> Option<String> {
    let wanted: &[&str] = if allow {
        &["allow_once", "allow_always"]
    } else {
        &["reject_once", "reject_always"]
    };
    for want in wanted {
        if let Some(opt) = options
            .iter()
            .find(|opt| opt.get("kind").and_then(|v| v.as_str()) == Some(*want))
        {
            if let Some(id) = opt.get("optionId").and_then(|v| v.as_str()) {
                return Some(id.to_string());
            }
        }
    }
    None
}

/// Parse model catalog + current selection from an ACP `session/new` or `session/load` result.
pub fn parse_acp_session_models(result: &Value) -> (Vec<String>, Option<String>) {
    if let Some(models) = result.get("models") {
        let current = models
            .get("currentModelId")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let ids: Vec<String> = models
            .get("availableModels")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m.get("modelId").and_then(|v| v.as_str()))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if !ids.is_empty() {
            return (ids, current);
        }
    }

    let Some(opts) = result.get("configOptions").and_then(|v| v.as_array()) else {
        return (vec![], None);
    };
    for opt in opts {
        if opt.get("id").and_then(|v| v.as_str()) != Some("model") {
            continue;
        }
        let current = opt
            .get("currentValue")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut ids = Vec::new();
        if let Some(options) = opt.get("options").and_then(|v| v.as_array()) {
            for o in options {
                if let Some(s) = o.as_str() {
                    ids.push(s.to_string());
                } else if let Some(v) = o.get("value").and_then(|v| v.as_str()) {
                    ids.push(v.to_string());
                } else if let Some(id) = o.get("id").and_then(|v| v.as_str()) {
                    ids.push(id.to_string());
                }
            }
        }
        return (ids, current);
    }
    (vec![], None)
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
        // ACP spec: `optionId` is agent-defined, not a fixed literal. Cache the
        // `params.options` arrays so `send_permission_response` can pick the real
        // optionId by `kind`.
        let pending_permission_options: Arc<Mutex<HashMap<String, Vec<Value>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let client_stdin = client.stdin_arc();

        client.spawn_stderr_reader(stderr);

        let tx = event_tx.clone();
        let pp = pending_permissions.clone();
        let ppo = pending_permission_options.clone();
        let turn_done = Arc::new(AtomicBool::new(false));
        let turn_had_content = Arc::new(AtomicBool::new(false));
        let last_activity = Arc::new(StdMutex::new(Instant::now()));
        // session/load replays prior conversation via session/update before its
        // response; the gateway renders history from its own JSONL, so replayed
        // chunks must not be re-emitted as live agent output.
        let replaying = Arc::new(AtomicBool::new(false));
        let turn_done_notify = turn_done.clone();
        let turn_had_content_notify = turn_had_content.clone();
        let last_activity_notify = last_activity.clone();
        let stdin_for_notify = client_stdin.clone();
        let replaying_notify = replaying.clone();
        let hooks_for_notify = hooks;
        let ppo_for_notify = ppo.clone();
        let on_notification: NotificationHandler = Arc::new(move |msg: &Value| {
            if let Ok(mut at) = last_activity_notify.lock() {
                *at = Instant::now();
            }
            let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
            let ctx = AcpNotifyCtx {
                event_tx: tx.clone(),
                pending_permissions: pp.clone(),
                pending_permission_options: ppo_for_notify.clone(),
                turn_done_sent: turn_done_notify.clone(),
                turn_had_content: turn_had_content_notify.clone(),
                client_stdin: stdin_for_notify.clone(),
            };
            match method {
                "session/update" => {
                    if replaying_notify.load(std::sync::atomic::Ordering::SeqCst) {
                        debug!("Suppressed replayed session/update during session/load");
                        return;
                    }
                    if let Some(update) = msg.get("params").and_then(|p| p.get("update")) {
                        handle_session_update(
                            update,
                            &ctx.event_tx,
                            &ctx.turn_done_sent,
                            Some(&ctx.turn_had_content),
                        );
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
            turn_had_content,
            last_activity,
            mcp_servers,
            available_models: vec![],
            active_model: None,
            hooks,
            pending_permission_options: ppo,
        };

        session
            .spawn_request(
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

        if let Some(method_id) = session.hooks.authenticate_method_id() {
            session
                .spawn_request("authenticate", json!({ "methodId": method_id }))
                .await?;
        }

        let mode = acp_mode(config);
        let will_resume = resume_session_id.is_some();
        session
            .hooks
            .before_session_setup(&event_tx, config, will_resume);

        let (result, loaded_session_id) = if let Some(ref sid) = resume_session_id {
            replaying.store(true, std::sync::atomic::Ordering::SeqCst);
            let load_result = session
                .spawn_request(
                    "session/load",
                    json!({
                        "sessionId": sid,
                        "cwd": work_dir,
                        "mode": mode,
                        "mcpServers": session.mcp_servers
                    }),
                )
                .await;
            replaying.store(false, std::sync::atomic::Ordering::SeqCst);
            match load_result {
                Ok(v) => (v, Some(sid.clone())),
                Err(e) => {
                    // session/load can fail when the provider doesn't implement it or
                    // the session has expired. Fall back to session/new so the spawn
                    // succeeds and the user gets a fresh session instead of a hard error.
                    tracing::warn!(
                        "[{}] session/load for {sid} failed: {e}; falling back to session/new",
                        session.hooks.log_provider_name()
                    );
                    let v = session
                        .spawn_request(
                            "session/new",
                            json!({
                                "cwd": work_dir,
                                "mode": mode,
                                "mcpServers": session.mcp_servers
                            }),
                        )
                        .await?;
                    (v, None)
                }
            }
        } else {
            let v = session
                .spawn_request(
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
        session.apply_session_models_from_result(&result);
        let _ = event_tx.send(AgentEvent::SessionId(session_id.clone()));

        session.hooks.after_session_setup(&session, config).await;

        Ok((session, Some(session_id)))
    }

    pub async fn send_user_message(&self, text: &str) -> Result<()> {
        reset_acp_turn_done(&self.turn_done_sent);
        reset_acp_turn_content(&self.turn_had_content);
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
        let turn_had_content = self.turn_had_content.clone();
        let last_activity = self.last_activity.clone();
        let stderr_buf = self.client.stderr_buffer();
        let closed_err = self.hooks.prompt_channel_closed_error().to_string();
        let no_output_msg = self.hooks.no_output_message();
        if let Ok(mut at) = last_activity.lock() {
            *at = Instant::now();
        }
        tokio::spawn(async move {
            use std::sync::atomic::Ordering;
            match await_prompt_response(rx, last_activity, PROMPT_IDLE_TIMEOUT, PROMPT_IDLE_TICK)
                .await
            {
                PromptWait::Completed(Ok(result)) => {
                    if !turn_had_content.load(Ordering::SeqCst) {
                        let stderr = stderr_buf
                            .lock()
                            .map(|lines| lines.join("\n"))
                            .unwrap_or_default();
                        let msg = resolve_acp_empty_turn_message(&result, &stderr, &no_output_msg);
                        tracing::warn!(
                            "ACP prompt completed with no streamed output (stopReason={:?})",
                            result.get("stopReason")
                        );
                        let _ = event_tx.send(AgentEvent::Error(msg));
                    }
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                PromptWait::Completed(Err(err)) => {
                    let _ = event_tx.send(AgentEvent::Error(err));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                PromptWait::ChannelClosed => {
                    let _ = event_tx.send(AgentEvent::Error(closed_err));
                    emit_acp_turn_done(&event_tx, &turn_done);
                }
                PromptWait::IdleTimeout => {
                    tracing::warn!(
                        "ACP prompt watchdog: no inbound traffic for {:?}, ending turn",
                        PROMPT_IDLE_TIMEOUT
                    );
                    let _ = event_tx.send(AgentEvent::Error(
                        crate::t!("agent.acp_prompt_idle_timeout").to_string(),
                    ));
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
        self.apply_session_models_from_result(&result);
        let _ = self
            .event_tx
            .send(AgentEvent::SessionId(session_id.clone()));
        Ok(Some(session_id))
    }

    fn apply_session_models_from_result(&mut self, result: &Value) {
        let (models, current) = parse_acp_session_models(result);
        self.available_models = models;
        self.active_model = current;
    }

    pub fn session_model_catalog(&self) -> &[String] {
        &self.available_models
    }

    pub fn session_active_model(&self) -> Option<&str> {
        self.active_model.as_deref()
    }

    pub(crate) fn set_session_active_model(&mut self, model_id: &str) {
        self.active_model = Some(model_id.to_string());
    }

    pub async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()> {
        // 1. Look up the JSON-RPC id (preserved verbatim so string / numeric shapes round-trip).
        let id_value = self
            .client
            .pending_permissions()
            .lock()
            .await
            .remove(request_id)
            .unwrap_or_else(|| Value::String(request_id.to_string()));

        // 2. Look up the cached `params.options` for this request, then pick the
        //    optionId whose `kind` matches the user's intent. Per the ACP spec,
        //    `optionId` is **agent-defined** — real-world values we've observed:
        //      codex-acp:   "approved" / "abort"
        //      gemini:      "proceed_always" / "proceed_once" / "cancel"
        //      qoderclicn:  "proceed_always_and_save" / "proceed_once" / "cancel"
        //    None of them use the old hard-coded literals `"once"` / `"reject"`.
        let cached_options = self
            .pending_permission_options
            .lock()
            .await
            .remove(request_id)
            .unwrap_or_default();
        let option_id = pick_option_id(&cached_options, allow).unwrap_or_else(|| {
            // Fallback for providers that don't send a conformant `options` array
            // (Cursor historically; defensive default).
            tracing::warn!(
                "[{}] no ACP option with kind {:?} in cached options; falling back to literal",
                self.hooks.log_provider_name(),
                if allow {
                    &["allow_once", "allow_always"][..]
                } else {
                    &["reject_once", "reject_always"][..]
                },
            );
            (if allow { "once" } else { "reject" }).to_string()
        });
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

    /// Spawn-time RPC: attach recent stderr so subscription/auth failures surface in chat.
    async fn spawn_request(&self, method: &str, params: Value) -> Result<Value> {
        self.send_request(method, params)
            .await
            .map_err(|e| append_client_stderr(&self.client, e))
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

    fn authenticate_method_id(&self) -> Option<&str> {
        None
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

    fn handle_extension_notification(
        &self,
        _method: &str,
        _msg: &Value,
        _ctx: &AcpNotifyCtx,
    ) -> bool {
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

    // Re-apply gateway-level `--yolo` semantics to the spawned CLI.
    //
    // The gateway strips `--yolo` from `default_args` and maps it to
    // `permission: allow` (see `parse_gateway_default_args` in model.rs).
    // Some providers need an explicit CLI flag to actually auto-approve
    // tool executions — e.g. qoderclicn requires `--permission-mode
    // bypass_permissions` in ACP mode, otherwise it still emits
    // `session/request_permission` notifications even when the gateway
    // auto-allows them.
    //
    // Claude is handled separately: its `--dangerously-skip-permissions`
    // flag is in the `DefaultArgsPolicy::Claude` passthrough list, so the
    // user's original flag arrives at the CLI verbatim.
    let caps = crate::config::agent_registry::capabilities_for(&config.provider);
    if !caps.yolo_cli_tokens.is_empty() && config.permission == "allow" {
        let already_present = args
            .windows(caps.yolo_cli_tokens.len())
            .any(|w| w.iter().zip(caps.yolo_cli_tokens).all(|(a, b)| a == *b));
        if !already_present {
            args.extend(caps.yolo_cli_tokens.iter().map(|s| (*s).to_string()));
        }
    }

    args
}

/// Outcome of waiting for a `session/prompt` response.
#[derive(Debug)]
pub(crate) enum PromptWait {
    /// The prompt RPC completed (turn ended) — `Ok(result)` or provider `Err(message)`.
    Completed(std::result::Result<Value, String>),
    /// The response channel dropped (client/process torn down).
    ChannelClosed,
    /// No inbound ACP traffic for `idle_timeout` — provider considered hung.
    IdleTimeout,
}

/// Wait for the prompt response with an **activity-based** watchdog: the turn may run
/// arbitrarily long as long as the provider keeps sending notifications; only a fully
/// silent provider trips `IdleTimeout`.
pub(crate) async fn await_prompt_response(
    mut rx: tokio::sync::oneshot::Receiver<std::result::Result<Value, String>>,
    last_activity: Arc<StdMutex<Instant>>,
    idle_timeout: Duration,
    tick: Duration,
) -> PromptWait {
    loop {
        match tokio::time::timeout(tick, &mut rx).await {
            Ok(Ok(result)) => return PromptWait::Completed(result),
            Ok(Err(_)) => return PromptWait::ChannelClosed,
            Err(_) => {
                let idle = last_activity
                    .lock()
                    .map(|at| at.elapsed())
                    .unwrap_or(idle_timeout);
                if idle >= idle_timeout {
                    return PromptWait::IdleTimeout;
                }
            }
        }
    }
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

/// Extract a human-readable label for a permission request from ACP `params`.
///
/// Per the ACP v1 spec the `toolCall` field is a `ToolCallUpdate` with fields
/// `toolCallId` / `title` / `kind` / `status` / `content` / `locations` / `rawInput`.
/// Real-world observations:
///
/// | Provider    | Fields present on `toolCall`                                  |
/// |-------------|---------------------------------------------------------------|
/// | codex-acp   | `toolCallId`, `kind`, `status`, `title`, `content`, `locations`, `rawInput` |
/// | gemini      | `toolCallId`, `status`, `title`, `content`, `locations`, `kind` |
/// | qoderclicn  | `toolCallId`, `status`, `title`, `content`, `kind`, `rawInput`, `_meta` |
/// | cursor      | historically exposes `name` as an extension field             |
///
/// Resolution order (best label first):
///   1. `toolCall.title`              ← primary human-readable description
///   2. `toolCall.name`               ← legacy / Cursor extension
///   3. `toolCall.toolCallId`         ← stable id; ugly but always present
///   4. `permission.title` / `permission.name` / `permission.toolName` / `permission.id`
///   5. top-level `title`
pub fn extract_permission_label(params: &Value) -> Option<String> {
    params
        .get("toolCall")
        .and_then(|v| {
            v.get("title")
                .or_else(|| v.get("name"))
                .or_else(|| v.get("toolCallId"))
        })
        .and_then(|v| v.as_str())
        .or_else(|| {
            params
                .get("permission")
                .and_then(|v| {
                    v.get("title")
                        .or_else(|| v.get("name"))
                        .or_else(|| v.get("toolName"))
                        .or_else(|| v.get("tool_name"))
                        .or_else(|| v.get("id"))
                })
                .and_then(|v| v.as_str())
        })
        .or_else(|| params.get("title").and_then(|v| v.as_str()))
        .map(|s| s.to_string())
}

fn handle_session_request_permission(msg: &Value, ctx: &AcpNotifyCtx, default_label: &str) {
    let Some(id) = msg.get("id").cloned() else {
        return;
    };
    let key = rpc_id_key(&id);
    let pp = ctx.pending_permissions.clone();
    let ppo = ctx.pending_permission_options.clone();
    let key2 = key.clone();
    let key3 = key.clone();
    // Cache the JSON-RPC id so `send_permission_response` can reply using the original id shape.
    tokio::spawn(async move {
        pp.lock().await.insert(key2, id);
    });
    let params = msg.get("params").cloned();
    // Cache `params.options` (ACP spec: agent-defined `optionId`s) separately so we
    // can later pick the right optionId by `kind` (allow_once / reject_once / ...).
    let options_vec = params
        .as_ref()
        .and_then(|p| p.get("options"))
        .and_then(|o| o.as_array())
        .cloned()
        .unwrap_or_default();
    tokio::spawn(async move {
        ppo.lock().await.insert(key3, options_vec);
    });
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

fn append_client_stderr(client: &AcpClient, err: anyhow::Error) -> anyhow::Error {
    let stderr_buf = client.recent_stderr();
    let stderr = stderr_buf.trim();
    if stderr.is_empty() {
        err
    } else {
        anyhow::anyhow!("{err}\n{stderr}")
    }
}

pub fn handle_session_update(
    update: &Value,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    turn_done_sent: &AtomicBool,
    turn_had_content: Option<&AtomicBool>,
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
        mark_acp_turn_content(turn_had_content);
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
        if !text.is_empty() {
            let _ = event_tx.send(AgentEvent::Text(text.to_string()));
            mark_acp_turn_content(turn_had_content);
        }
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
        mark_acp_turn_content(turn_had_content);
    } else if kind.contains("error") {
        let _ = event_tx.send(AgentEvent::Error(update.to_string()));
        mark_acp_turn_content(turn_had_content);
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
            None,
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
            None,
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
            None,
        );
        assert!(matches!(rx.try_recv(), Ok(AgentEvent::Done)));
    }

    #[test]
    fn extracts_permission_label_prefers_toolcall_title_over_name() {
        // 新优先级：`title` 优先于 `name`（实测 codex/gemini/qoder 都只给 `title`，
        // `name` 是少数老 provider / Cursor 扩展才带）。
        let params = json!({
            "toolCall": { "name": "mcp__cc-gateway__send_file", "title": "Send file" }
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("Send file".to_string())
        );
    }

    #[test]
    fn extracts_permission_label_codex_real_payload() {
        // 实测 codex-acp 2026-06 抓到的真实 permission request params。
        let params = json!({
            "sessionId": "019ebc5f-...",
            "toolCall": {
                "toolCallId": "call_cdP26WY56b6i5Mvnav2jxFx1",
                "kind": "edit",
                "status": "pending",
                "title": "Edit /tmp/acp-probe-codex/acp-probe-test.txt",
                "content": [{ "type": "diff", "path": "/tmp/acp-probe-codex/acp-probe-test.txt", "newText": "hello from acp probe\n" }],
                "locations": [{ "path": "/tmp/acp-probe-codex/acp-probe-test.txt" }],
                "rawInput": { "call_id": "call_cdP26WY56b6i5Mvnav2jxFx1" }
            },
            "options": [
                { "optionId": "approved", "name": "Yes", "kind": "allow_once" },
                { "optionId": "abort", "name": "No, provide feedback", "kind": "reject_once" }
            ]
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("Edit /tmp/acp-probe-codex/acp-probe-test.txt".to_string())
        );
    }

    #[test]
    fn extracts_permission_label_gemini_real_payload() {
        // 实测 gemini --acp 2026-06 抓到的真实 permission request params。
        let params = json!({
            "toolCall": {
                "toolCallId": "call_xyz",
                "status": "pending",
                "title": "Writing to acp-probe-test.txt",
                "content": [],
                "locations": [],
                "kind": "edit"
            },
            "options": [
                { "optionId": "proceed_always", "kind": "allow_always", "name": "Allow for this session" },
                { "optionId": "proceed_once", "kind": "allow_once", "name": "Allow" },
                { "optionId": "cancel", "kind": "reject_once", "name": "Reject" }
            ]
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("Writing to acp-probe-test.txt".to_string())
        );
    }

    #[test]
    fn extracts_permission_label_qoder_real_payload() {
        // 实测 qoderclicn --acp 2026-06 抓到的真实 permission request params。
        let params = json!({
            "toolCall": {
                "toolCallId": "call_abc",
                "status": "pending",
                "title": "echo probe-ok > /tmp/acp-probe-shell.txt",
                "content": [],
                "kind": "execute",
                "rawInput": {},
                "_meta": {}
            },
            "options": [
                { "optionId": "proceed_always_and_save", "kind": "allow_always", "name": "Always allow \"echo\"" },
                { "optionId": "proceed_once", "kind": "allow_once", "name": "Allow" },
                { "optionId": "cancel", "kind": "reject_once", "name": "Reject" }
            ]
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("echo probe-ok > /tmp/acp-probe-shell.txt".to_string())
        );
    }

    #[test]
    fn extracts_permission_label_falls_back_to_tool_call_id() {
        // title / name 都没有时退到 toolCallId（保证一定有 label）。
        let params = json!({
            "toolCall": { "toolCallId": "call_deadbeef", "kind": "edit" }
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("call_deadbeef".to_string())
        );
    }

    #[test]
    fn resolve_permission_option_id_picks_by_kind() {
        // send_permission_response 的"按 kind 选 optionId"核心逻辑的纯函数版。
        let options = vec![
            json!({ "optionId": "proceed_always", "kind": "allow_always", "name": "Allow for this session" }),
            json!({ "optionId": "proceed_once",   "kind": "allow_once",   "name": "Allow" }),
            json!({ "optionId": "cancel",         "kind": "reject_once",  "name": "Reject" }),
        ];
        assert_eq!(
            pick_option_id(&options, true).as_deref(),
            Some("proceed_once")
        );
        assert_eq!(pick_option_id(&options, false).as_deref(), Some("cancel"));
    }

    #[test]
    fn resolve_permission_option_id_codex_approved_abort() {
        // codex-acp 实测选项：allow 应该选 "approved"，deny 应该选 "abort"。
        let options = vec![
            json!({ "optionId": "approved", "kind": "allow_once", "name": "Yes" }),
            json!({ "optionId": "abort",    "kind": "reject_once", "name": "No, provide feedback" }),
        ];
        assert_eq!(pick_option_id(&options, true).as_deref(), Some("approved"));
        assert_eq!(pick_option_id(&options, false).as_deref(), Some("abort"));
    }

    #[test]
    fn resolve_permission_option_id_qoder_proceed_always_and_save() {
        // qoderclicn 多一个 `allow_always` 变体，但 allow_once 应当优先（最小权限）。
        let options = vec![
            json!({ "optionId": "proceed_always_and_save", "kind": "allow_always", "name": "Always allow" }),
            json!({ "optionId": "proceed_once",             "kind": "allow_once",   "name": "Allow" }),
            json!({ "optionId": "cancel",                   "kind": "reject_once",  "name": "Reject" }),
        ];
        assert_eq!(
            pick_option_id(&options, true).as_deref(),
            Some("proceed_once")
        );
    }

    #[test]
    fn resolve_permission_option_id_falls_back_when_no_kind_match() {
        // 未知 kind 时返回 None，由调用方决定 fallback 字面量。
        let options = vec![json!({ "optionId": "weird_yes", "kind": "maybe", "name": "Weird" })];
        assert_eq!(pick_option_id(&options, true), None);
        assert_eq!(pick_option_id(&options, false), None);
    }

    #[test]
    fn parse_acp_session_models_reads_config_options() {
        let result = json!({
            "sessionId": "s1",
            "configOptions": [{
                "id": "model",
                "currentValue": "gpt-5.4-mini",
                "options": ["gpt-5.5", "gpt-5.4-mini"]
            }]
        });
        let (models, current) = parse_acp_session_models(&result);
        assert_eq!(models, vec!["gpt-5.5", "gpt-5.4-mini"]);
        assert_eq!(current.as_deref(), Some("gpt-5.4-mini"));
    }

    #[test]
    fn parse_acp_session_models_reads_gemini_models_field() {
        let result = json!({
            "sessionId": "s1",
            "models": {
                "currentModelId": "auto",
                "availableModels": [
                    { "modelId": "auto", "name": "Auto" },
                    { "modelId": "gemini-3-flash-preview", "name": "gemini-3-flash-preview" }
                ]
            }
        });
        let (models, current) = parse_acp_session_models(&result);
        assert_eq!(models, vec!["auto", "gemini-3-flash-preview"]);
        assert_eq!(current.as_deref(), Some("auto"));
    }

    fn activity_at_now() -> Arc<StdMutex<Instant>> {
        Arc::new(StdMutex::new(Instant::now()))
    }

    #[tokio::test]
    async fn prompt_wait_completes_when_response_arrives_late() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            let _ = tx.send(Ok(json!({"stopReason": "end_turn"})));
        });
        // idle_timeout far larger than the response delay: no false trigger.
        let outcome = await_prompt_response(
            rx,
            activity_at_now(),
            Duration::from_secs(60),
            Duration::from_millis(5),
        )
        .await;
        assert!(matches!(outcome, PromptWait::Completed(Ok(_))));
    }

    #[tokio::test]
    async fn prompt_wait_times_out_only_when_provider_fully_silent() {
        let (_tx, rx) = tokio::sync::oneshot::channel::<std::result::Result<Value, String>>();
        let outcome = await_prompt_response(
            rx,
            activity_at_now(),
            Duration::from_millis(50),
            Duration::from_millis(10),
        )
        .await;
        assert!(matches!(outcome, PromptWait::IdleTimeout));
    }

    #[tokio::test]
    async fn prompt_wait_survives_long_turn_with_ongoing_activity() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let activity = activity_at_now();
        let activity_refresher = activity.clone();
        // Simulate a turn 4x longer than idle_timeout that keeps streaming updates.
        tokio::spawn(async move {
            for _ in 0..20 {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if let Ok(mut at) = activity_refresher.lock() {
                    *at = Instant::now();
                }
            }
            let _ = tx.send(Ok(json!({"stopReason": "end_turn"})));
        });
        let outcome = await_prompt_response(
            rx,
            activity,
            Duration::from_millis(50),
            Duration::from_millis(5),
        )
        .await;
        assert!(matches!(outcome, PromptWait::Completed(Ok(_))));
    }

    #[tokio::test]
    async fn prompt_wait_reports_channel_closed_when_client_drops() {
        let (tx, rx) = tokio::sync::oneshot::channel::<std::result::Result<Value, String>>();
        drop(tx);
        let outcome = await_prompt_response(
            rx,
            activity_at_now(),
            Duration::from_secs(60),
            Duration::from_millis(5),
        )
        .await;
        assert!(matches!(outcome, PromptWait::ChannelClosed));
    }
}
