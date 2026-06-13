//! Agent session layer: Encapsulates concrete backend types for different providers (Claude stream-json, Codex /
//! Cursor / OpenCode / Kimi / Gemini / **Qoder** ACP, Pi JSON-RPC) into a single
//! [`AgentRuntime`] enum, exposing unified interfaces such as `spawn`, `send_message`, and `stop`.
//!
//! This module acts as the "dispatch layer" between the [`crate::core::agent::backend::AgentBackend`] trait and upper-level business logic
//! ([`crate::core::runtime::controller::AgentController`]):
//!
//! - Business logic only interacts with `AgentRuntime` and is unaware of the concrete backend types;
//! - The unified event stream across all providers is described by [`AgentEvent`];
//! - Claude uses its own stream-json protocol and requires [`stream_json_to_agent_events`] for event translation;
//!   other providers directly produce [`AgentEvent`] within their respective backends.

use anyhow::Result;
use tokio::sync::mpsc;

use crate::agent::backend::AgentBackend;
use crate::agent::codex_acp::CodexAcpSession;
use crate::agent::cursor_acp::CursorAcpSession;
use crate::agent::event::{AgentEvent, QuestionItem, QuestionOption};
use crate::agent::gemini_acp::GeminiAcpSession;
use crate::agent::kimi_acp::KimiAcpSession;
use crate::agent::mcp_attach::{log_unsupported_mcp, supports_mcp_attach};
use crate::agent::opencode_acp::OpenCodeAcpSession;
use crate::agent::pi_rpc::PiRpcSession;
use crate::agent::qoder_acp::QoderAcpSession;
use crate::config::model::{AgentConfig, AgentProvider};
use crate::runtime::mcp_server::McpContext;
use crate::runtime::protocol::{InputMessage, OutputEvent};
use crate::runtime::session::StreamJsonSession;

/// Context parameters required to start a "fresh provider session" (without recreating the gateway record).
///
/// Used for commands like `/clear`: stops the current provider session and starts a new one within
/// the same agent session, while preserving the work_dir, config, and MCP context.
pub struct NewProviderSessionCtx<'a> {
    pub work_dir: String,
    pub config: &'a AgentConfig,
    pub extra_args: Vec<String>,
    pub mcp_context: Option<McpContext>,
}

/// Currently running agent session.
///
/// Each variant holds a provider backend instance (e.g., [`CodexAcpSession`],
/// [`StreamJsonSession`]). Upper-level business logic only interacts with [`AgentRuntime`], and concrete dispatching
/// is handled via the [`crate::dispatch_agent_backend!`] macro or a `match` expression.
///
/// **Lifecycle**: Owned by [`crate::core::runtime::controller::AgentController`]. When a session ends
/// (`/quit` or user disconnects), calls [`Self::stop`] for a graceful exit or [`Self::force_stop`]
/// to terminate immediately.
pub enum AgentRuntime {
    Claude(StreamJsonSession),
    Codex(CodexAcpSession),
    Cursor(CursorAcpSession),
    Pi(PiRpcSession),
    OpenCode(OpenCodeAcpSession),
    Kimi(KimiAcpSession),
    Gemini(GeminiAcpSession),
    Qoder(QoderAcpSession),
}

impl AgentRuntime {
    /// Maps the enum variant back to the [`AgentProvider`] enum, simplifying capability lookups and MCP injection policies.
    pub fn provider_kind(&self) -> AgentProvider {
        match self {
            Self::Claude(_) => AgentProvider::Claude,
            Self::Codex(_) => AgentProvider::Codex,
            Self::Cursor(_) => AgentProvider::Cursor,
            Self::Pi(_) => AgentProvider::Pi,
            Self::OpenCode(_) => AgentProvider::OpenCode,
            Self::Kimi(_) => AgentProvider::Kimi,
            Self::Gemini(_) => AgentProvider::Gemini,
            Self::Qoder(_) => AgentProvider::Qoder,
        }
    }

    /// Spawns the corresponding backend child process according to [`AgentConfig::provider`].
    ///
    /// Flow (generic for all providers):
    ///
    /// 1. If `mcp_context` is not empty but the provider does not support MCP injection, log a warning (does not block spawn);
    /// 2. Evaluate the match branch and invoke the static `spawn` method of the respective backend;
    /// 3. Return `(AgentRuntime, Option<provider_session_id>)`: the session ID is persisted in
    ///    SQLite and passed back to the backend during subsequent session restorations.
    ///
    /// The Claude branch is unique: it runs on the stream-json protocol and spawns an additional task to translate
    /// [`OutputEvent`]s emitted by the stdout reader into [`AgentEvent`]s, pushing them to `event_tx`.
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
        if mcp_context.is_some() && !supports_mcp_attach(config.provider.clone()) {
            log_unsupported_mcp(config.provider.clone());
        }
        match config.provider {
            AgentProvider::Claude => {
                let (claude_tx, mut claude_rx) = mpsc::unbounded_channel::<OutputEvent>();
                let (session, session_id) = StreamJsonSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    claude_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                let tx = event_tx.clone();
                tokio::spawn(async move {
                    while let Some(event) = claude_rx.recv().await {
                        for agent_event in stream_json_to_agent_events(event) {
                            let _ = tx.send(agent_event);
                        }
                    }
                });
                Ok((Self::Claude(session), session_id))
            }
            AgentProvider::Codex => {
                let (session, session_id) = CodexAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::Codex(session), session_id))
            }
            AgentProvider::Cursor => {
                let (session, session_id) = CursorAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::Cursor(session), session_id))
            }
            AgentProvider::Pi => {
                let (session, session_id) = PiRpcSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::Pi(session), session_id))
            }
            AgentProvider::OpenCode => {
                let (session, session_id) = OpenCodeAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::OpenCode(session), session_id))
            }
            AgentProvider::Kimi => {
                let (session, session_id) = KimiAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::Kimi(session), session_id))
            }
            AgentProvider::Gemini => {
                let (session, session_id) = GeminiAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::Gemini(session), session_id))
            }
            AgentProvider::Qoder => {
                let (session, session_id) = QoderAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                Ok((Self::Qoder(session), session_id))
            }
        }
    }

    /// Forwards a user text message to the backend (ACP `session/prompt` / Claude stream-json
    /// `user_message` / Pi JSON-RPC `send_message`).
    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::send_user_message(b, text).await)
    }

    /// Flushes queued pending user messages to the backend (used only by Claude to handle stream-json's
    /// `interrupt` frame; other providers use a default empty implementation).
    pub async fn flush_queued_messages(&mut self) -> Result<()> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::flush_queued_messages(b).await)
    }

    /// Sends `/stop`: Aborts the current generation while keeping the session process alive.
    pub async fn send_stop_generation(&mut self) -> Result<()> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::send_stop_generation(b).await)
    }

    /// Switches the model used by the current session. Returns the newly activated model ID (or an empty string
    /// if the provider does not return a response).
    pub async fn set_model(&mut self, provider: &AgentProvider, model_id: &str) -> Result<String> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::set_model(b, provider, model_id)
            .await)
    }

    /// `/compact`: Clear context / compact history.
    ///
    /// - Claude: Sends `/compact` as a user message (`compact_via_user_message = true`);
    /// - ACP providers: Starts a new session via `session/new` carrying the instructions;
    /// - Pi: Invokes the JSON-RPC `compact_context` method.
    ///
    /// The return value is the new provider session ID (if any), written back to SQLite by the caller for subsequent restoration.
    pub async fn compact_context(&mut self, instructions: Option<&str>) -> Result<String> {
        let provider = self.provider_kind();
        crate::dispatch_agent_backend!(self, |b| AgentBackend::compact_context(
            b,
            instructions,
            provider
        )
        .await)
    }

    /// Starts a fresh provider session (ACP `session/new` / Pi `new_session` / Claude
    /// respawn). Used for `/clear`.
    pub async fn new_provider_session(
        &mut self,
        ctx: &NewProviderSessionCtx<'_>,
    ) -> Result<Option<String>> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::new_provider_session(b, ctx).await)
    }

    /// Sends structured input (user messages / permission responses).
    pub async fn send_input(&mut self, msg: InputMessage) -> Result<()> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::send_input(b, msg).await)
    }

    /// Queries the currently active model ID (returns `None` if the provider does not respond).
    pub async fn active_model_id(&mut self) -> Result<Option<String>> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::active_model_id(b).await)
    }

    /// Lists the model IDs currently available for the provider.
    pub async fn list_available_models_in_session(&mut self) -> Result<Vec<String>> {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::list_available_models_in_session(b)
            .await)
    }

    /// Gracefully closes the session: sends `session/cancel` / closes stdin / waits for child process exit.
    pub async fn stop(self) -> Result<()> {
        match self {
            Self::Claude(session) => session.stop().await,
            Self::Codex(session) => session.stop().await,
            Self::Cursor(session) => session.stop().await,
            Self::Pi(session) => session.stop().await,
            Self::OpenCode(session) => session.stop().await,
            Self::Kimi(session) => session.stop().await,
            Self::Gemini(session) => session.stop().await,
            Self::Qoder(session) => session.stop().await,
        }
    }

    /// Forcefully terminates the provider child process (`SIGTERM` / Windows `taskkill`).
    ///
    /// Used only for gateway control commands (`/quit`) — these commands must respond immediately
    /// without blocking the UI even if the provider child process is hanging.
    pub async fn force_stop(self) -> Result<()> {
        match self {
            Self::Claude(session) => session.force_stop().await,
            Self::Codex(session) => session.force_stop().await,
            Self::Cursor(session) => session.force_stop().await,
            Self::Pi(session) => session.force_stop().await,
            Self::OpenCode(session) => session.force_stop().await,
            Self::Kimi(session) => session.force_stop().await,
            Self::Gemini(session) => session.force_stop().await,
            Self::Qoder(session) => session.force_stop().await,
        }
    }

    /// Whether the backend child process is still alive.
    pub fn is_alive(&mut self) -> bool {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::is_alive(b))
    }

    /// Recent stderr output of the backend (used for the error troubleshooting panel).
    pub fn recent_stderr(&mut self) -> String {
        crate::dispatch_agent_backend!(self, |b| AgentBackend::recent_stderr(b))
    }
}

/// Translates Claude-exclusive stream-json protocol frames into unified [`AgentEvent`]s.
///
/// Other providers directly produce [`AgentEvent`]s in their respective backends; **only Claude routes through here**.
fn stream_json_to_agent_events(event: OutputEvent) -> Vec<AgentEvent> {
    match event {
        OutputEvent::System { session_id } => {
            session_id.map(AgentEvent::SessionId).into_iter().collect()
        }
        OutputEvent::Assistant { message } => message
            .content
            .into_iter()
            .map(|block| match block {
                crate::runtime::protocol::ContentBlock::Text { text } => AgentEvent::Text(text),
                crate::runtime::protocol::ContentBlock::Thinking { thinking } => {
                    AgentEvent::Thinking(thinking)
                }
                crate::runtime::protocol::ContentBlock::ToolUse { name, input } => {
                    AgentEvent::ToolUse(name, serde_json::to_string(&input).unwrap_or_default())
                }
                crate::runtime::protocol::ContentBlock::ToolResult { content, is_error } => {
                    AgentEvent::ToolResult(content.unwrap_or_default(), is_error)
                }
                crate::runtime::protocol::ContentBlock::Image { source } => {
                    AgentEvent::Text(format!(
                        "[Image: {} {} ({} bytes)]",
                        source.source_type,
                        source.media_type,
                        source.data.len()
                    ))
                }
            })
            .collect(),
        OutputEvent::Result { usage, .. } => {
            if let Some(u) = usage {
                tracing::debug!(
                    "Usage: input={} output={}",
                    u.input_tokens.unwrap_or(0),
                    u.output_tokens.unwrap_or(0)
                );
            }
            vec![AgentEvent::Done]
        }
        OutputEvent::ControlRequest {
            request_id,
            request,
        } => vec![claude_control_request_to_agent_event(request_id, request)],
        OutputEvent::Error { error } => vec![AgentEvent::Error(error)],
        OutputEvent::User { .. } => Vec::new(),
    }
}

/// Translates Claude's control requests (permissions, AskUserQuestion, confirm, select_option)
/// into [`AgentEvent`]s.
fn claude_control_request_to_agent_event(
    request_id: String,
    request: crate::runtime::protocol::ControlRequestBody,
) -> AgentEvent {
    let tool_name = request
        .tool_name
        .clone()
        .unwrap_or_else(|| request.subtype.clone());
    let input = request.input.clone();

    // `AskUserQuestion`: Claude's built-in tool to ask users multiple questions — parsed into
    // `AgentEvent::QuestionRequest` and rendered as cards or keyboards by the chat layer.
    if tool_name == "AskUserQuestion" {
        if let Some(ref val) = input {
            if let Some(questions) = val.get("questions").and_then(|q| q.as_array()) {
                let parsed: Vec<QuestionItem> = questions
                    .iter()
                    .filter_map(|q| {
                        let question = q.get("question")?.as_str()?.to_string();
                        let header = q.get("header")?.as_str()?.to_string();
                        let multi_select = q
                            .get("multi_select")
                            .and_then(|m| m.as_bool())
                            .unwrap_or(false);
                        let options = q.get("options")?.as_array()?;
                        let parsed_options: Vec<QuestionOption> = options
                            .iter()
                            .filter_map(|o| {
                                let label = o.get("label")?.as_str()?.to_string();
                                let description = o.get("description")?.as_str()?.to_string();
                                Some(QuestionOption { label, description })
                            })
                            .collect();
                        Some(QuestionItem {
                            question,
                            header,
                            options: parsed_options,
                            multi_select,
                        })
                    })
                    .collect();
                if !parsed.is_empty() {
                    return AgentEvent::QuestionRequest {
                        request_id,
                        questions: parsed,
                    };
                }
            }
        }
    }

    // `confirm`: A confirmation dialog with a single prompt and multiple options.
    if request.subtype == "confirm" {
        if let Some(ref val) = input {
            let prompt = val
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let options: Vec<String> = val
                .get("options")
                .and_then(|o| o.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            return AgentEvent::ConfirmRequest {
                request_id,
                prompt,
                options,
            };
        }
    }

    // `select_option`: A select option dialog.
    if request.subtype == "select_option" {
        if let Some(ref val) = input {
            let prompt = val
                .get("prompt")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            let options: Vec<String> = val
                .get("options")
                .and_then(|o| o.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            return AgentEvent::SelectRequest {
                request_id,
                prompt,
                options,
            };
        }
    }

    // All others are treated as permission requests (Claude uses `control_request` to ask for permissions before tool execution).
    AgentEvent::PermissionRequest {
        request_id,
        tool_name,
        input,
    }
}
