//! Unified "gateway-side session API" exposed by all providers.
//!
//! Each provider backend (Claude stream-json, Codex / Cursor / OpenCode / Kimi /
//! Gemini / **Qoder** ACP, Pi JSON-RPC) implements the [`AgentBackend`] trait.
//! [`super::session::AgentRuntime`] dispatches to the concrete implementation via the `dispatch_agent_backend!` macro.
//! Thus, adding a new provider only requires:
//!
//! 1. Adding a new enum variant in [`super::session::AgentRuntime`];
//! 2. Implementing `AgentBackend` (ACP providers automatically obtain a blanket impl via [`GenericAcpSession`], no manual coding needed);
//! 3. Adding a new branch in the `dispatch_agent_backend!` macro.

use anyhow::Result;
use async_trait::async_trait;

use crate::agent::acp_session::GenericAcpSession;
use crate::agent::pi_rpc::PiRpcSession;
use crate::config::model::AgentProvider;
use crate::runtime::protocol::{build_user_message, InputMessage};
use crate::runtime::session::StreamJsonSession;

use super::session::NewProviderSessionCtx;

/// Unified capability interface for all provider backends.
///
/// The default trait implementation provides graceful degradation (returns "not supported" errors for
/// `compact_context`, `set_model`, `list_available_models_in_session`, `active_model_id`, `recent_stderr`).
/// Concrete backends override them as needed.
#[async_trait]
pub trait AgentBackend {
    /// Send a user text message to the provider.
    async fn send_user_message(&mut self, text: &str) -> Result<()>;

    /// `/stop`: Abort the current generation while keeping the session process alive.
    async fn send_stop_generation(&mut self) -> Result<()>;

    /// Send structured input: user messages, permission responses, confirmations, or choices.
    async fn send_input(&mut self, msg: InputMessage) -> Result<()>;

    /// `/compact`: Clear context / compact history.
    ///
    /// The default implementation returns a `compact_not_supported` error; Claude, Pi, and ACP providers
    /// each override this implementation (Claude sends `/compact` as a user message; ACP starts a new session; Pi uses RPC).
    ///
    /// The return value is the new provider session ID (if any), written back to SQLite by the caller.
    async fn compact_context(
        &mut self,
        _instructions: Option<&str>,
        provider: AgentProvider,
    ) -> Result<String> {
        anyhow::bail!(
            "{}",
            crate::t_fmt!(
                "builtin.compact_not_supported",
                NAME = crate::command::agents::provider_display_name(&provider)
            )
        )
    }

    /// `/clear`: Start a fresh provider session (without recreating the gateway record).
    ///
    /// Defaults to returning `None` (indicating the backend does not expose a new session ID); ACP providers override
    /// this to return the session ID from `session/new` for persistence.
    async fn new_provider_session(
        &mut self,
        _ctx: &NewProviderSessionCtx<'_>,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    /// `/models`: Switch model inside the session.
    ///
    /// The default implementation returns different errors based on the provider's [`AgentCapabilities::platform_bound`]
    /// ("platform-bound, model cannot be switched in WebUI" / "this provider does not support model switching");
    /// providers supporting in-session model switching (Claude respawn with `--resume … --model`,
    /// Kimi / Gemini / OpenCode / Codex / Qoder ACP `session/set_model`, Pi RPC `set_model`) override this.
    async fn set_model(&mut self, provider: &AgentProvider, _model_id: &str) -> Result<String> {
        let caps = crate::config::agent_registry::capabilities_for(provider);
        if caps.platform_bound {
            anyhow::bail!(
                "{}",
                crate::t_fmt!(
                    "models.not_supported_platform_agent",
                    NAME = crate::command::agents::provider_display_name(provider)
                )
            );
        }
        anyhow::bail!(
            "{}",
            crate::t_fmt!(
                "models.not_supported",
                NAME = crate::command::agents::provider_display_name(provider)
            )
        )
    }

    /// Query the currently active model ID. Overridden by Pi's `get_state`, others default to `None`.
    async fn active_model_id(&mut self) -> Result<Option<String>> {
        Ok(None)
    }

    /// List the model IDs currently available for the provider (used by the `/models` command).
    async fn list_available_models_in_session(&mut self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    /// Whether the backend child process is still alive (used for WebUI / bot status badges).
    fn is_alive(&mut self) -> bool;

    /// Recent stderr output (used for the error troubleshooting panel; defaults to an empty string).
    fn recent_stderr(&self) -> String {
        String::new()
    }
}

/// Capability interface for backends supporting "permission responses" (providers **other than** Claude stream-json).
///
/// Claude uses its own control request/response protocol and does not need this trait;
/// ACP and Pi implement this trait, and [`send_permission_capable_input`] translates the unified
/// [`InputMessage::ControlResponse`] into the provider's "allow / deny" semantics.
#[async_trait]
pub trait PermissionCapableBackend: AgentBackend {
    /// Send permission response: `allow = true` permits the action, `false` denies it.
    async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()>;
}

/// Dispatches [`InputMessage`] to the backend supporting permission responses:
///
/// - [`InputMessage::ControlResponse`]: User's response to permission prompts -> calls
///   [`PermissionCapableBackend::send_permission_response`];
/// - [`InputMessage::User`]: Regular user message -> calls [`AgentBackend::send_user_message`].
async fn send_permission_capable_input<B: PermissionCapableBackend + ?Sized>(
    backend: &mut B,
    msg: InputMessage,
) -> Result<()> {
    match msg {
        InputMessage::ControlResponse { response } => {
            let allow = response.response.behavior == "allow";
            backend
                .send_permission_response(&response.request_id, allow)
                .await
        }
        InputMessage::User { message } => {
            let text = message
                .content
                .as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| message.content.to_string());
            backend.send_user_message(&text).await
        }
        // ACP / Pi cancel via their own `send_cancel`; the stream-json control frame is a no-op here.
        InputMessage::ControlRequest { .. } => Ok(()),
    }
}

#[async_trait]
impl AgentBackend for StreamJsonSession {
    async fn send_user_message(&mut self, text: &str) -> Result<()> {
        self.send(build_user_message(text)).await
    }

    async fn send_stop_generation(&mut self) -> Result<()> {
        // `/stop` must cancel the running turn via a `control_request` so the session can keep
        // responding afterwards. A bare `{"type":"interrupt"}` is a no-op in headless stream-json
        // mode (Claude never acks it), which left the session unable to answer the next message.
        let request_id = format!("stop-{}", uuid::Uuid::new_v4());
        self.send(crate::runtime::protocol::build_interrupt_request(&request_id))
            .await
    }

    async fn send_input(&mut self, msg: InputMessage) -> Result<()> {
        self.send(msg).await
    }

    async fn compact_context(
        &mut self,
        instructions: Option<&str>,
        _provider: AgentProvider,
    ) -> Result<String> {
        let text = match instructions.filter(|s| !s.trim().is_empty()) {
            Some(hint) => format!("/compact {hint}"),
            None => "/compact".to_string(),
        };
        self.send(build_user_message(&text)).await?;
        Ok(String::new())
    }

    async fn new_provider_session(
        &mut self,
        ctx: &NewProviderSessionCtx<'_>,
    ) -> Result<Option<String>> {
        self.restart_fresh(ctx.extra_args.clone(), ctx.config, ctx.mcp_context.clone())
            .await
    }

    async fn set_model(&mut self, _provider: &AgentProvider, model_id: &str) -> Result<String> {
        StreamJsonSession::set_model(self, model_id).await
    }

    fn is_alive(&mut self) -> bool {
        StreamJsonSession::is_alive(self)
    }

    fn recent_stderr(&self) -> String {
        StreamJsonSession::recent_stderr(self)
    }
}

#[async_trait]
impl<H: crate::agent::acp_session::AcpHooks> AgentBackend for GenericAcpSession<H> {
    async fn send_user_message(&mut self, text: &str) -> Result<()> {
        GenericAcpSession::send_user_message(self, text).await
    }

    async fn set_model(&mut self, provider: &AgentProvider, model_id: &str) -> Result<String> {
        if self.hooks.supports_acp_set_model() {
            self.hooks.set_session_model(self, model_id).await?;
            self.set_session_active_model(model_id);
            return Ok(model_id.to_string());
        }
        let caps = crate::config::agent_registry::capabilities_for(provider);
        if caps.platform_bound {
            anyhow::bail!(
                "{}",
                crate::t_fmt!(
                    "models.not_supported_platform_agent",
                    NAME = crate::command::agents::provider_display_name(provider)
                )
            );
        }
        anyhow::bail!(
            "{}",
            crate::t_fmt!(
                "models.not_supported",
                NAME = crate::command::agents::provider_display_name(provider)
            )
        )
    }

    async fn send_stop_generation(&mut self) -> Result<()> {
        GenericAcpSession::send_cancel(self).await
    }

    async fn send_input(&mut self, msg: InputMessage) -> Result<()> {
        send_permission_capable_input(self, msg).await
    }

    async fn new_provider_session(
        &mut self,
        ctx: &NewProviderSessionCtx<'_>,
    ) -> Result<Option<String>> {
        GenericAcpSession::new_provider_session(self, &ctx.work_dir, ctx.config).await
    }

    async fn active_model_id(&mut self) -> Result<Option<String>> {
        Ok(self.session_active_model().map(str::to_string))
    }

    async fn list_available_models_in_session(&mut self) -> Result<Vec<String>> {
        Ok(self.session_model_catalog().to_vec())
    }

    fn is_alive(&mut self) -> bool {
        GenericAcpSession::is_alive(self)
    }

    fn recent_stderr(&self) -> String {
        GenericAcpSession::recent_stderr(self)
    }
}

#[async_trait]
impl<H: crate::agent::acp_session::AcpHooks> PermissionCapableBackend for GenericAcpSession<H> {
    async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()> {
        GenericAcpSession::send_permission_response(self, request_id, allow).await
    }
}

#[async_trait]
impl AgentBackend for PiRpcSession {
    async fn send_user_message(&mut self, text: &str) -> Result<()> {
        PiRpcSession::send_user_message(self, text).await
    }

    async fn send_stop_generation(&mut self) -> Result<()> {
        PiRpcSession::send_cancel(self).await
    }

    async fn send_input(&mut self, msg: InputMessage) -> Result<()> {
        send_permission_capable_input(self, msg).await
    }

    async fn compact_context(
        &mut self,
        instructions: Option<&str>,
        _provider: AgentProvider,
    ) -> Result<String> {
        PiRpcSession::compact_context(self, instructions).await
    }

    async fn new_provider_session(
        &mut self,
        _ctx: &NewProviderSessionCtx<'_>,
    ) -> Result<Option<String>> {
        PiRpcSession::new_provider_session(self).await
    }

    async fn set_model(&mut self, _provider: &AgentProvider, model_id: &str) -> Result<String> {
        let Some((p, mid)) = crate::command::models::parse_provider_model_id(model_id) else {
            anyhow::bail!("{}", crate::t!("models.pi_requires_provider_model"));
        };
        PiRpcSession::set_model(self, &p, &mid).await
    }

    async fn active_model_id(&mut self) -> Result<Option<String>> {
        PiRpcSession::active_model_id(self).await
    }

    async fn list_available_models_in_session(&mut self) -> Result<Vec<String>> {
        PiRpcSession::get_available_models(self).await
    }

    fn is_alive(&mut self) -> bool {
        PiRpcSession::is_alive(self)
    }

    fn recent_stderr(&self) -> String {
        PiRpcSession::recent_stderr(self)
    }
}

#[async_trait]
impl PermissionCapableBackend for PiRpcSession {
    async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()> {
        PiRpcSession::send_permission_response(self, request_id, allow).await
    }
}

/// Unified macro to dispatch [`super::session::AgentRuntime`] to its internal backend.
///
/// Usage:
///
/// ```ignore
/// crate::dispatch_agent_backend!(self, |b| b.send_user_message(text).await)
/// ```
///
/// The expanded macro produces a `match` expression: binds each variant of `$self` (Claude / Codex / Cursor / Pi /
/// OpenCode / Kimi / Gemini / Qoder) to `$b` and executes `$body`.
/// This allows all [`AgentRuntime`] methods requiring "per-provider dispatching" to reuse the same codebase,
/// meaning adding a new provider **only requires adding a variant branch here**.
#[macro_export]
macro_rules! dispatch_agent_backend {
    ($self:expr, |$b:ident| $body:expr) => {
        match $self {
            $crate::agent::session::AgentRuntime::Claude($b) => $body,
            $crate::agent::session::AgentRuntime::Codex($b) => $body,
            $crate::agent::session::AgentRuntime::Cursor($b) => $body,
            $crate::agent::session::AgentRuntime::Pi($b) => $body,
            $crate::agent::session::AgentRuntime::OpenCode($b) => $body,
            $crate::agent::session::AgentRuntime::Kimi($b) => $body,
            $crate::agent::session::AgentRuntime::Gemini($b) => $body,
            $crate::agent::session::AgentRuntime::Qoder($b) => $body,
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::protocol::build_permission_allow;

    struct StubBackend {
        last_message: Option<String>,
    }

    #[async_trait]
    impl AgentBackend for StubBackend {
        async fn send_user_message(&mut self, text: &str) -> Result<()> {
            self.last_message = Some(text.to_string());
            Ok(())
        }

        async fn send_stop_generation(&mut self) -> Result<()> {
            Ok(())
        }

        async fn send_input(&mut self, msg: InputMessage) -> Result<()> {
            send_permission_capable_input(self, msg).await
        }

        fn is_alive(&mut self) -> bool {
            true
        }
    }

    #[async_trait]
    impl PermissionCapableBackend for StubBackend {
        async fn send_permission_response(&self, _request_id: &str, _allow: bool) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn permission_capable_input_forwards_user_text() {
        let mut backend = StubBackend { last_message: None };
        backend
            .send_input(build_user_message("hello"))
            .await
            .expect("user input should forward");
        assert_eq!(backend.last_message.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn permission_capable_input_forwards_allow() {
        let mut backend = StubBackend { last_message: None };
        backend
            .send_input(build_permission_allow("req-1", None))
            .await
            .expect("permission allow should forward");
    }
}
