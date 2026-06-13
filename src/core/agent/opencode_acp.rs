use anyhow::Result;
use serde_json::{json, Value};

use crate::agent::acp_session::{build_base_spawn_args, AcpHooks, GenericAcpSession};
use crate::agent::mcp_attach::build_acp_mcp_servers;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

/// OpenCode ACP client (`opencode acp` — NDJSON JSON-RPC over stdio).
pub type OpenCodeAcpSession = GenericAcpSession<OpenCodeAcpHooks>;

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenCodeAcpHooks;

#[async_trait::async_trait]
impl AcpHooks for OpenCodeAcpHooks {
    fn log_provider_name(&self) -> &'static str {
        "OpenCode"
    }

    fn authenticate_method_id(&self) -> Option<&str> {
        Some("opencode-login")
    }

    fn default_permission_label(&self) -> &'static str {
        "opencode_permission"
    }

    fn prompt_channel_closed_error(&self) -> &'static str {
        "OpenCode ACP prompt response channel closed"
    }

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn OpenCode ACP. Is '{}' installed and on PATH? Tried '{} acp'.",
            config.cli_path, cli_path
        )
    }

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}",
            crate::t_fmt!("opencode.session_resume_failed", ID = session_id, ERR = err)
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

    fn supports_acp_set_model(&self) -> bool {
        true
    }

    async fn set_session_model(&self, session: &OpenCodeAcpSession, model_id: &str) -> Result<()> {
        let params = json!({
            "sessionId": session.acp_session_id(),
            "modelId": model_id
        });
        match session
            .acp_request("session/set_model", params.clone())
            .await
        {
            Ok(_) => Ok(()),
            Err(e) if e.to_string().contains("Method not found") => {
                session
                    .acp_request("unstable_setSessionModel", params)
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
    use crate::agent::acp_client::resolve_acp_spawn_session_id;
    use crate::agent::acp_session::{extract_permission_label, handle_session_update};
    use crate::agent::event::AgentEvent;
    use std::sync::atomic::AtomicBool;
    use tokio::sync::mpsc;

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
            None,
        );

        match rx.try_recv().expect("event should be sent") {
            AgentEvent::Text(text) => assert_eq!(text, "hello"),
            other => panic!("expected text event, got {:?}", other),
        }
    }

    #[test]
    fn opencode_session_resume_error_is_user_visible() {
        let err = OpenCodeAcpHooks::session_resume_error("sess-1", "timeout");
        let msg = err.to_string();
        assert!(msg.contains("sess-1"));
        assert!(msg.contains("timeout"));
    }

    #[test]
    fn extracts_permission_label_when_only_toolcall_name() {
        // OpenCode 扩展历史路径：只给 `name` 没给 `title` 时仍能用 `name` 兜底。
        let params = json!({
            "toolCall": { "name": "mcp__cc-gateway__send_file" }
        });
        assert_eq!(
            extract_permission_label(&params),
            Some("mcp__cc-gateway__send_file".to_string())
        );
    }
}
