use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::acp_session::{build_base_spawn_args, AcpHooks, GenericAcpSession};
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// Gemini CLI ACP client (`gemini --acp` — NDJSON JSON-RPC over stdio).
///
/// Gemini has no usable `authenticate` flow for a headless gateway (its
/// authMethods are `oauth-personal` / `gemini-api-key` / `vertex-ai`, which
/// trigger interactive login). The CLI reuses cached credentials, so we skip
/// `authenticate` and let `session/new` fail with a sign-in hint when the
/// user has never logged in.
pub type GeminiAcpSession = GenericAcpSession<GeminiAcpHooks>;

#[derive(Debug, Clone, Copy, Default)]
pub struct GeminiAcpHooks;

#[async_trait::async_trait]
impl AcpHooks for GeminiAcpHooks {
    fn log_provider_name(&self) -> &'static str {
        "Gemini"
    }

    fn authenticate_method_id(&self) -> Option<&str> {
        None
    }

    fn default_permission_label(&self) -> &'static str {
        "gemini_permission"
    }

    fn prompt_channel_closed_error(&self) -> &'static str {
        "Gemini ACP prompt response channel closed"
    }

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn Gemini CLI. Is '{}' installed and on PATH? Tried '{} --acp'.",
            config.cli_path, cli_path
        )
    }

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}",
            crate::t_fmt!("gemini.session_resume_failed", ID = session_id, ERR = err)
        )
    }

    fn normalize_work_dir(work_dir: &str) -> Result<String> {
        crate::runtime::controller::ensure_under_home(work_dir)
    }

    async fn prepare_mcp_servers(
        _work_dir: &str,
        mcp_context: Option<&McpContext>,
    ) -> Result<Value> {
        build_acp_mcp_servers(mcp_context)
    }

    fn build_spawn_args(
        config: &AgentConfig,
        extra_args: Vec<String>,
        _mcp_servers: &Value,
    ) -> Vec<String> {
        let mut args = build_base_spawn_args(config, extra_args);
        args.push("--acp".to_string());
        args
    }

    fn handle_extension_notification(
        &self,
        _method: &str,
        _msg: &Value,
        _ctx: &crate::agent::acp_session::AcpNotifyCtx,
    ) -> bool {
        false
    }

    fn supports_acp_set_model(&self) -> bool {
        true
    }

    async fn set_session_model(&self, session: &GeminiAcpSession, model_id: &str) -> Result<()> {
        session
            .acp_request(
                "session/set_model",
                json!({
                    "sessionId": session.acp_session_id(),
                    "modelId": model_id
                }),
            )
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::AgentProvider;

    #[test]
    fn spawn_args_use_acp_flag_not_positional_subcommand() {
        let config = AgentConfig::default_for_provider(AgentProvider::Gemini);
        let args = GeminiAcpHooks::build_spawn_args(&config, vec![], &Value::Null);
        assert_eq!(args, vec!["--acp".to_string()]);
        assert!(!args.contains(&"acp".to_string()));
    }

    #[test]
    fn spawn_args_keep_default_and_extra_args_before_acp_flag() {
        // Verifies ordering: default_args → extra_args → `--acp`.
        // Note: `--yolo` is stripped by `parse_gateway_default_args` before this function is
        // called in production, so we use a neutral flag here to avoid implying `--yolo`
        // reaches the Gemini CLI (it doesn't — gateway auto-approves via `permission: allow`).
        let mut config = AgentConfig::default_for_provider(AgentProvider::Gemini);
        config.default_args = "--sandbox".to_string();
        let args = GeminiAcpHooks::build_spawn_args(
            &config,
            vec!["-m".into(), "auto".into()],
            &Value::Null,
        );
        assert_eq!(args, vec!["--sandbox", "-m", "auto", "--acp"]);
    }

    #[test]
    fn authenticate_is_skipped() {
        // Gemini rejects unknown authenticate methods; cached oauth needs no authenticate RPC.
        assert_eq!(AcpHooks::authenticate_method_id(&GeminiAcpHooks), None);
    }

    #[test]
    fn gemini_session_resume_error_is_user_visible() {
        let err = GeminiAcpHooks::session_resume_error("sess-1", "timeout");
        let msg = err.to_string();
        assert!(msg.contains("sess-1"));
        assert!(msg.contains("timeout"));
    }
}
