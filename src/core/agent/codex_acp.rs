use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::acp_session::{build_base_spawn_args, AcpHooks, GenericAcpSession};
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// Codex ACP client (`codex-acp` — Zed's adapter wrapping the Codex CLI,
/// NDJSON JSON-RPC over stdio). Install with `npm i -g @zed-industries/codex-acp`;
/// auth is shared with the `codex` CLI (`codex login` or `OPENAI_API_KEY`).
///
/// Like Gemini, the adapter's authMethods (`chatgpt`, API-key env vars) are
/// interactive or environment-driven, so the `authenticate` RPC is skipped and
/// cached credentials are used. `session/new` ignores the `mode` param; the
/// configured mode is applied afterwards via `session/set_mode`
/// ([`AcpHooks::after_session_setup`]). Model switch uses
/// `session/set_config_option` (`configId: "model"`).
pub type CodexAcpSession = GenericAcpSession<CodexAcpHooks>;

/// Modes advertised by codex-acp (`read-only` is its own default).
const CODEX_MODES: &[&str] = &["read-only", "auto", "full-access"];

/// Maps the gateway `--yolo` flag to `full-access` mode.
///
/// When the user sets `--yolo` in `default_args`, `parse_gateway_default_args` strips
/// the flag and sets `permission = "allow"`. Codex has no `--yolo` CLI flag, so the
/// equivalent intent is expressed by upgrading the provider mode to `full-access`.
fn effective_mode(config: &AgentConfig) -> &str {
    if config.permission == "allow" {
        "full-access"
    } else {
        config.mode.trim()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CodexAcpHooks;

#[async_trait::async_trait]
impl AcpHooks for CodexAcpHooks {
    fn log_provider_name(&self) -> &'static str {
        "Codex"
    }

    fn authenticate_method_id(&self) -> Option<&str> {
        None
    }

    fn default_permission_label(&self) -> &'static str {
        "codex_permission"
    }

    fn prompt_channel_closed_error(&self) -> &'static str {
        "Codex ACP prompt response channel closed"
    }

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn Codex ACP adapter. Is '{}' installed and on PATH? \
             Install with `npm i -g @zed-industries/codex-acp` (tried '{}').",
            config.cli_path, cli_path
        )
    }

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}",
            crate::t_fmt!("codex.session_resume_failed", ID = session_id, ERR = err)
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
        // codex-acp speaks ACP directly; no subcommand or flag needed.
        build_base_spawn_args(config, extra_args)
    }

    fn handle_extension_notification(
        &self,
        _method: &str,
        _msg: &Value,
        _ctx: &crate::agent::acp_session::AcpNotifyCtx,
    ) -> bool {
        false
    }

    async fn after_session_setup(&self, session: &CodexAcpSession, config: &AgentConfig) {
        let mode = effective_mode(config);
        if !CODEX_MODES.contains(&mode) {
            return;
        }
        if let Err(e) = session
            .acp_request(
                "session/set_mode",
                json!({
                    "sessionId": session.acp_session_id(),
                    "modeId": mode
                }),
            )
            .await
        {
            tracing::warn!("Codex session/set_mode '{}' failed: {}", mode, e);
        }
    }

    fn supports_acp_set_model(&self) -> bool {
        true
    }

    async fn set_session_model(&self, session: &CodexAcpSession, model_id: &str) -> Result<()> {
        session
            .acp_request(
                "session/set_config_option",
                json!({
                    "sessionId": session.acp_session_id(),
                    "configId": "model",
                    "value": model_id
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
    fn spawn_args_have_no_subcommand() {
        let config = AgentConfig::default_for_provider(AgentProvider::Codex);
        let args = CodexAcpHooks::build_spawn_args(&config, vec![], &Value::Null);
        assert!(args.is_empty());
    }

    #[test]
    fn spawn_args_keep_default_and_extra_args() {
        let mut config = AgentConfig::default_for_provider(AgentProvider::Codex);
        config.default_args = "-c model=gpt-5.5".to_string();
        let args = CodexAcpHooks::build_spawn_args(&config, vec!["--enable".into()], &Value::Null);
        assert_eq!(args, vec!["-c", "model=gpt-5.5", "--enable"]);
    }

    #[test]
    fn authenticate_is_skipped() {
        // codex-acp authMethods are interactive/env-var; cached codex login is reused.
        assert_eq!(AcpHooks::authenticate_method_id(&CodexAcpHooks), None);
    }

    #[test]
    fn default_mode_is_a_valid_codex_mode() {
        let config = AgentConfig::default_for_provider(AgentProvider::Codex);
        assert!(CODEX_MODES.contains(&config.mode.as_str()));
    }

    #[test]
    fn codex_session_resume_error_is_user_visible() {
        let err = CodexAcpHooks::session_resume_error("sess-1", "timeout");
        let msg = err.to_string();
        assert!(msg.contains("sess-1"));
        assert!(msg.contains("timeout"));
    }

    /// Gateway-level `--yolo` (stripped to `permission: allow`) must map to
    /// `full-access` mode, since codex-acp has no `--yolo` CLI flag.
    #[test]
    fn codex_yolo_maps_to_full_access_mode() {
        let mut config = AgentConfig::default_for_provider(AgentProvider::Codex);
        config.default_args = String::new();
        config.permission = "allow".to_string();
        assert_eq!(effective_mode(&config), "full-access");
    }

    /// Without `--yolo`, the explicitly-configured mode is used unchanged.
    #[test]
    fn codex_explicit_mode_respected_without_yolo() {
        let mut config = AgentConfig::default_for_provider(AgentProvider::Codex);
        config.mode = "read-only".to_string();
        config.permission = "prompt".to_string();
        assert_eq!(effective_mode(&config), "read-only");
    }

    /// `--yolo` overrides an explicit `mode` setting (full-access is the stronger intent).
    #[test]
    fn codex_yolo_overrides_explicit_mode() {
        let mut config = AgentConfig::default_for_provider(AgentProvider::Codex);
        config.mode = "read-only".to_string();
        config.permission = "allow".to_string();
        assert_eq!(effective_mode(&config), "full-access");
    }
}
