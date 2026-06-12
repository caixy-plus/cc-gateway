use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::acp_session::{build_base_spawn_args, AcpHooks, GenericAcpSession};
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// Kimi Code CLI ACP client (`kimi acp` — NDJSON JSON-RPC over stdio).
pub type KimiAcpSession = GenericAcpSession<KimiAcpHooks>;

#[derive(Debug, Clone, Copy, Default)]
pub struct KimiAcpHooks;

#[async_trait::async_trait]
impl AcpHooks for KimiAcpHooks {
    fn log_provider_name(&self) -> &'static str {
        "Kimi"
    }

    fn authenticate_method_id(&self) -> Option<&str> {
        Some("login")
    }

    fn default_permission_label(&self) -> &'static str {
        "kimi_permission"
    }

    fn prompt_channel_closed_error(&self) -> &'static str {
        "Kimi ACP prompt response channel closed"
    }

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn Kimi Code CLI. Is '{}' installed and on PATH? Tried '{} acp'.",
            config.cli_path, cli_path
        )
    }

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}",
            crate::t_fmt!("kimi.session_resume_failed", ID = session_id, ERR = err)
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
        args.push("acp".to_string());
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

    fn no_output_message(&self) -> String {
        crate::t!("agent.kimi_no_response").to_string()
    }

    fn supports_acp_set_model(&self) -> bool {
        true
    }

    async fn set_session_model(&self, session: &KimiAcpSession, model_id: &str) -> Result<()> {
        let session_id = session.acp_session_id();
        let set_model_params = json!({
            "sessionId": session_id,
            "modelId": model_id
        });
        match session
            .acp_request("session/set_model", set_model_params.clone())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("Method not found") => {
                session
                    .acp_request(
                        "session/set_config_option",
                        json!({
                            "sessionId": session_id,
                            "configId": "model",
                            "value": model_id
                        }),
                    )
                    .await?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::acp_session::{extract_permission_label, handle_session_update};
    use crate::agent::event::AgentEvent;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::mpsc;

    #[test]
    fn maps_kimi_text_update_to_agent_event() {
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
    }

    #[test]
    fn kimi_session_resume_error_is_user_visible() {
        let err = KimiAcpHooks::session_resume_error("sess-1", "timeout");
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
    fn authenticate_method_id_is_login() {
        let hooks = KimiAcpHooks;
        assert_eq!(AcpHooks::authenticate_method_id(&hooks), Some("login"));
    }
}
