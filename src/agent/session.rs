use anyhow::Result;
use tokio::sync::mpsc;

use crate::agent::cursor_acp::CursorAcpSession;
use crate::agent::event::{AgentEvent, QuestionItem, QuestionOption};
use crate::claude::mcp_server::McpContext;
use crate::claude::protocol::{InputMessage, OutputEvent};
use crate::claude::session::ClaudeSession;
use crate::config::model::{AgentConfig, AgentProvider};

pub enum AgentSession {
    Claude(ClaudeSession),
    Cursor(CursorAcpSession),
}

impl AgentSession {
    pub async fn spawn(
        work_dir: String,
        extra_args: Vec<String>,
        config: &AgentConfig,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> Result<(Self, Option<String>)> {
        match config.provider {
            AgentProvider::Claude => {
                let (claude_tx, mut claude_rx) = mpsc::unbounded_channel::<OutputEvent>();
                let claude_config: crate::config::model::ClaudeConfig = config.clone().into();
                let (session, session_id) = ClaudeSession::spawn(
                    work_dir,
                    extra_args,
                    &claude_config,
                    claude_tx,
                    resume_session_id,
                    mcp_context,
                )
                .await?;
                let tx = event_tx.clone();
                tokio::spawn(async move {
                    while let Some(event) = claude_rx.recv().await {
                        for agent_event in claude_to_agent_events(event) {
                            let _ = tx.send(agent_event);
                        }
                    }
                });
                Ok((Self::Claude(session), session_id))
            }
            AgentProvider::Cursor => {
                let (session, session_id) = CursorAcpSession::spawn(
                    work_dir,
                    extra_args,
                    config,
                    event_tx,
                    resume_session_id,
                )
                .await?;
                Ok((Self::Cursor(session), session_id))
            }
        }
    }

    pub async fn send_message(&mut self, text: &str) -> Result<()> {
        match self {
            AgentSession::Claude(session) => {
                session
                    .send(crate::claude::protocol::build_user_message(text))
                    .await
            }
            AgentSession::Cursor(session) => session.send_user_message(text).await,
        }
    }

    pub async fn send_input(&mut self, msg: InputMessage) -> Result<()> {
        match self {
            AgentSession::Claude(session) => session.send(msg).await,
            AgentSession::Cursor(session) => match msg {
                InputMessage::ControlResponse { response } => {
                    let allow = response.response.behavior == "allow";
                    session
                        .send_permission_response(&response.request_id, allow)
                        .await
                }
                InputMessage::User { message } => {
                    let text = message
                        .content
                        .as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| message.content.to_string());
                    session.send_user_message(&text).await
                }
            },
        }
    }

    pub async fn stop(self) -> Result<()> {
        match self {
            AgentSession::Claude(session) => session.stop().await,
            AgentSession::Cursor(session) => session.stop().await,
        }
    }

    pub fn is_alive(&mut self) -> bool {
        match self {
            AgentSession::Claude(session) => session.is_alive(),
            AgentSession::Cursor(session) => session.is_alive(),
        }
    }
}

impl From<AgentConfig> for crate::config::model::ClaudeConfig {
    fn from(value: AgentConfig) -> Self {
        Self {
            cli_path: value.cli_path,
            default_args: value.default_args,
        }
    }
}

fn claude_to_agent_events(event: OutputEvent) -> Vec<AgentEvent> {
    match event {
        OutputEvent::System { session_id } => {
            session_id.map(AgentEvent::SessionId).into_iter().collect()
        }
        OutputEvent::Assistant { message } => message
            .content
            .into_iter()
            .map(|block| match block {
                crate::claude::protocol::ContentBlock::Text { text } => AgentEvent::Text(text),
                crate::claude::protocol::ContentBlock::Thinking { thinking } => {
                    AgentEvent::Thinking(thinking)
                }
                crate::claude::protocol::ContentBlock::ToolUse { name, input } => {
                    AgentEvent::ToolUse(name, serde_json::to_string(&input).unwrap_or_default())
                }
                crate::claude::protocol::ContentBlock::ToolResult { content, is_error } => {
                    AgentEvent::ToolResult(content.unwrap_or_default(), is_error)
                }
                crate::claude::protocol::ContentBlock::Image { source } => {
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

fn claude_control_request_to_agent_event(
    request_id: String,
    request: crate::claude::protocol::ControlRequestBody,
) -> AgentEvent {
    let tool_name = request
        .tool_name
        .clone()
        .unwrap_or_else(|| request.subtype.clone());
    let input = request.input.clone();

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

    AgentEvent::PermissionRequest {
        request_id,
        tool_name,
        input,
    }
}
