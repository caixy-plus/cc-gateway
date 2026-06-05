use serde_json::{json, Value};

use crate::agent::acp_session::{
    build_base_spawn_args, respond_acp_extension, AcpHooks, AcpNotifyCtx, GenericAcpSession,
};
use crate::agent::event::{AgentEvent, QuestionItem, QuestionOption};
use crate::agent::mcp_attach::prepare_cursor_mcp;
use crate::config::model::AgentConfig;
use crate::runtime::mcp_server::McpContext;

pub type CursorAcpSession = GenericAcpSession<CursorAcpHooks>;

#[derive(Debug, Clone, Copy, Default)]
pub struct CursorAcpHooks;

#[async_trait::async_trait]
impl AcpHooks for CursorAcpHooks {
    fn log_provider_name(&self) -> &'static str {
        "Cursor"
    }

    fn authenticate_method_id(&self) -> &str {
        "cursor_login"
    }

    fn default_permission_label(&self) -> &'static str {
        "cursor_permission"
    }

    fn prompt_channel_closed_error(&self) -> &'static str {
        "Cursor ACP prompt response channel closed"
    }

    fn spawn_failure_message(config: &AgentConfig, cli_path: &str) -> String {
        format!(
            "Failed to spawn Cursor Agent CLI. Is '{}' installed and on PATH? Tried '{}'.",
            config.cli_path, cli_path
        )
    }

    fn session_resume_error(session_id: &str, err: &str) -> anyhow::Error {
        anyhow::anyhow!(
            "{}",
            crate::t_fmt!("cursor.session_resume_failed", ID = session_id, ERR = err)
        )
    }

    async fn prepare_mcp_servers(
        work_dir: &str,
        mcp_context: Option<&McpContext>,
    ) -> anyhow::Result<Value> {
        prepare_cursor_mcp(work_dir, mcp_context).await
    }

    fn build_spawn_args(
        config: &AgentConfig,
        extra_args: Vec<String>,
        mcp_servers: &Value,
    ) -> Vec<String> {
        let mut args = build_base_spawn_args(config, extra_args);
        if !mcp_servers.as_array().is_some_and(|a| a.is_empty())
            && !args.iter().any(|a| a == "--approve-mcps")
        {
            args.push("--approve-mcps".to_string());
        }
        args.push("acp".to_string());
        args
    }

    fn before_session_setup(
        &self,
        event_tx: &tokio::sync::mpsc::UnboundedSender<AgentEvent>,
        config: &AgentConfig,
        will_resume: bool,
    ) {
        if will_resume
            && (config.default_args.contains("--yolo") || config.default_args.contains("--print"))
        {
            let _ = event_tx.send(AgentEvent::Text(
                crate::t!("cursor.resume_may_ignore_flags").to_string(),
            ));
        }
    }

    fn handle_extension_notification(&self, method: &str, msg: &Value, ctx: &AcpNotifyCtx) -> bool {
        match method {
            "cursor/ask_question" => {
                if let Some(params) = msg.get("params") {
                    if let Some(questions) = parse_cursor_questions(params) {
                        let request_id = msg
                            .get("id")
                            .map(crate::agent::acp_session::rpc_id_key)
                            .unwrap_or_else(|| "cursor-question".to_string());
                        let _ = ctx.event_tx.send(AgentEvent::QuestionRequest {
                            request_id,
                            questions,
                        });
                    }
                }
                respond_acp_extension(
                    &ctx.client_stdin,
                    msg,
                    json!({
                        "outcome": { "outcome": "skipped", "reason": "cc-gateway does not yet collect Cursor ACP question answers" }
                    }),
                );
                true
            }
            "cursor/create_plan" => {
                if let Some(plan) = msg
                    .get("params")
                    .and_then(|p| p.get("plan"))
                    .and_then(|p| p.as_str())
                {
                    let _ = ctx
                        .event_tx
                        .send(AgentEvent::Text(format!("\n[Plan requested]\n{}\n", plan)));
                }
                respond_acp_extension(
                    &ctx.client_stdin,
                    msg,
                    json!({
                        "outcome": { "outcome": "rejected", "reason": "Plan approval is not available through cc-gateway yet" }
                    }),
                );
                true
            }
            "cursor/update_todos" | "cursor/task" | "cursor/generate_image" => {
                tracing::debug!("Cursor ACP extension notification: {}", method);
                true
            }
            _ => false,
        }
    }
}

fn parse_cursor_questions(params: &Value) -> Option<Vec<QuestionItem>> {
    let questions = params.get("questions")?.as_array()?;
    let parsed: Vec<QuestionItem> = questions
        .iter()
        .filter_map(|q| {
            let question = q.get("prompt")?.as_str()?.to_string();
            let options = q.get("options")?.as_array()?;
            let parsed_options: Vec<QuestionOption> = options
                .iter()
                .filter_map(|o| {
                    Some(QuestionOption {
                        label: o.get("label")?.as_str()?.to_string(),
                        description: o
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect();
            Some(QuestionItem {
                question,
                header: params
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                options: parsed_options,
                multi_select: q
                    .get("allowMultiple")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            })
        })
        .collect();
    if parsed.is_empty() {
        None
    } else {
        Some(parsed)
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
    fn parses_cursor_question_extension_payload() {
        let questions = parse_cursor_questions(&json!({
            "title": "Need input",
            "questions": [{
                "id": "q1",
                "prompt": "Which mode?",
                "allowMultiple": false,
                "options": [
                    { "id": "agent", "label": "Agent" },
                    { "id": "plan", "label": "Plan" }
                ]
            }]
        }))
        .expect("question should parse");

        assert_eq!(questions.len(), 1);
        assert_eq!(questions[0].header, "Need input");
        assert_eq!(questions[0].question, "Which mode?");
        assert_eq!(questions[0].options[0].label, "Agent");
    }

    #[test]
    fn cursor_session_resume_error_is_user_visible() {
        let err = CursorAcpHooks::session_resume_error("abc-123", "Session not found");
        let msg = err.to_string();
        assert!(msg.contains("abc-123"));
        assert!(msg.contains("Session not found"));
    }

    #[test]
    fn maps_cursor_text_update_to_agent_event() {
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
    }

    #[test]
    fn maps_cursor_turn_complete_to_done() {
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

    #[tokio::test]
    async fn real_cursor_acp_smoke_test_when_enabled() {
        if std::env::var("CC_GATEWAY_RUN_CURSOR_AGENT_TEST")
            .ok()
            .as_deref()
            != Some("1")
        {
            return;
        }

        let cli_path = std::env::var("CC_GATEWAY_CURSOR_AGENT_PATH")
            .unwrap_or_else(|_| r"C:\Users\volun\AppData\Local\cursor-agent\agent.cmd".to_string());
        let config = AgentConfig {
            provider: crate::config::model::AgentProvider::Cursor,
            cli_path,
            default_args: String::new(),
            mode: "agent".to_string(),
            permission: "prompt".to_string(),
        };
        let (tx, _rx) = mpsc::unbounded_channel();
        let work_dir = std::env::current_dir()
            .expect("current dir should be available")
            .to_string_lossy()
            .to_string();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            CursorAcpSession::spawn(work_dir, Vec::new(), &config, tx, None, None),
        )
        .await
        .expect("Cursor ACP smoke test timed out")
        .expect("Cursor ACP session should start");

        let (session, session_id) = result;
        assert!(session_id.as_deref().unwrap_or("").len() > 8);
        session.stop().await.expect("session should stop");
    }
}
