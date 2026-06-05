//! Unified gateway-facing session API for all agent providers.
//!
//! Each concrete session type implements [`AgentBackend`]; [`super::session::AgentRuntime`]
//! dispatches through this trait so new providers add one enum variant and one `impl` block.

use anyhow::Result;
use async_trait::async_trait;

use crate::agent::acp_session::GenericAcpSession;
use crate::agent::pi_rpc::PiRpcSession;
use crate::config::model::AgentProvider;
use crate::runtime::protocol::{build_user_message, InputMessage};
use crate::runtime::session::StreamJsonSession;

use super::session::NewProviderSessionCtx;

/// Gateway operations shared by Claude, ACP, and RPC backends.
#[async_trait]
pub trait AgentBackend {
    async fn send_user_message(&mut self, text: &str) -> Result<()>;

    async fn flush_queued_messages(&mut self) -> Result<()> {
        Ok(())
    }

    async fn send_stop_generation(&mut self) -> Result<()>;

    async fn send_input(&mut self, msg: InputMessage) -> Result<()>;

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

    async fn new_provider_session(
        &mut self,
        _ctx: &NewProviderSessionCtx<'_>,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    async fn set_model(
        &mut self,
        provider: &AgentProvider,
        _model_id: &str,
    ) -> Result<String> {
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

    async fn active_model_id(&mut self) -> Result<Option<String>> {
        Ok(None)
    }

    async fn list_available_models_in_session(&mut self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    fn is_alive(&mut self) -> bool;

    fn recent_stderr(&self) -> String {
        String::new()
    }
}

/// Permission / user follow-up for backends that are not Claude stream-json.
#[async_trait]
pub trait PermissionCapableBackend: AgentBackend {
    async fn send_permission_response(&self, request_id: &str, allow: bool) -> Result<()>;
}

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
        InputMessage::Interrupt => Ok(()),
    }
}

#[async_trait]
impl AgentBackend for StreamJsonSession {
    async fn send_user_message(&mut self, text: &str) -> Result<()> {
        self.send(build_user_message(text)).await
    }

    async fn flush_queued_messages(&mut self) -> Result<()> {
        self.send(InputMessage::Interrupt).await
    }

    async fn send_stop_generation(&mut self) -> Result<()> {
        self.send(InputMessage::Interrupt).await
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

    async fn set_model(
        &mut self,
        provider: &AgentProvider,
        model_id: &str,
    ) -> Result<String> {
        if self.hooks.supports_acp_set_model() {
            self.hooks.set_session_model(self, model_id).await?;
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

    async fn set_model(
        &mut self,
        _provider: &AgentProvider,
        model_id: &str,
    ) -> Result<String> {
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

#[macro_export]
macro_rules! dispatch_agent_backend {
    ($self:expr, |$b:ident| $body:expr) => {
        match $self {
            $crate::agent::session::AgentRuntime::Claude($b) => $body,
            $crate::agent::session::AgentRuntime::Cursor($b) => $body,
            $crate::agent::session::AgentRuntime::Pi($b) => $body,
            $crate::agent::session::AgentRuntime::OpenCode($b) => $body,
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
        let mut backend = StubBackend {
            last_message: None,
        };
        backend
            .send_input(build_user_message("hello"))
            .await
            .expect("user input should forward");
        assert_eq!(backend.last_message.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn permission_capable_input_forwards_allow() {
        let mut backend = StubBackend {
            last_message: None,
        };
        backend
            .send_input(build_permission_allow("req-1", None))
            .await
            .expect("permission allow should forward");
    }
}
