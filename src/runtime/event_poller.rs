use anyhow::Result;
use serde_json::Value;
use tokio::sync::{mpsc, Mutex};
use tracing::debug;

use crate::runtime::controller::{AgentController, ControllerEvent};

/// Buffers assistant text for one agent turn; platforms flush on `Done` / errors.
#[derive(Default)]
pub struct TurnTextBuffer {
    inner: String,
}

impl TurnTextBuffer {
    pub fn push(&mut self, text: &str) {
        if !text.is_empty() {
            self.inner.push_str(text);
        }
    }

    pub fn take_nonempty(&mut self) -> Option<String> {
        if self.inner.trim().is_empty() {
            self.inner.clear();
            None
        } else {
            Some(std::mem::take(&mut self.inner))
        }
    }
}

/// Whether buffered assistant text should be delivered to the user now.
pub fn should_flush_turn_buffer(text: &str, is_done: bool) -> bool {
    is_done || text.starts_with("Error:")
}

/// Sink trait for consuming Claude events during a poll cycle.
/// Each platform/TUI provides its own implementation.
#[async_trait::async_trait]
pub trait EventPollSink: Send {
    /// Called when Claude emits a text/thinking/tool output chunk.
    /// `is_done` is true when the response stream for this turn is complete.
    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()>;

    /// Called when Claude requests permission for a tool.
    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        input: Option<&Value>,
    ) -> Result<()>;

    /// Called when Claude requests a confirmation.
    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()>;

    /// Called when Claude requests a selection.
    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()>;

    /// Called when Claude sends an AskUserQuestion form.
    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::runtime::controller::QuestionItem],
    ) -> Result<()>;
}

/// Polls AgentController events and dispatches them to an EventPollSink.
pub struct AgentEventPoller {
    event_rx: std::sync::Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>>,
}

impl AgentEventPoller {
    /// Create a poller from an existing controller reference.
    /// Clones the controller's event receiver so the poller can listen
    /// independently after the controller lock is released.
    pub fn from_controller(controller: &AgentController) -> Self {
        Self {
            event_rx: controller.event_rx_clone(),
        }
    }

    /// Run the poll loop: drain events from the controller and dispatch to `sink`.
    /// Returns when the session ends (Done event) or the event channel closes.
    pub async fn run(self, sink: &mut (dyn EventPollSink + Send)) -> Result<()> {
        let mut hidden_thinking_placeholder_sent = false;
        loop {
            let event = {
                let mut rx = self.event_rx.lock().await;
                rx.recv().await
            };

            match event {
                Some(ControllerEvent::Text(text)) => {
                    sink.flush(&text, false).await?;
                }
                Some(ControllerEvent::Thinking(text)) => {
                    if text.trim().is_empty() {
                        if !hidden_thinking_placeholder_sent {
                            sink.flush(crate::t!("claude.thinking_placeholder"), false)
                                .await?;
                            hidden_thinking_placeholder_sent = true;
                        }
                    } else {
                        sink.flush(&text, false).await?;
                    }
                }
                Some(ControllerEvent::ToolUse(name, input)) => {
                    let text = format!("\n[Tool: {}]\n{}\n", name, input);
                    sink.flush(&text, false).await?;
                }
                Some(ControllerEvent::ToolResult(text, is_error)) => {
                    let prefix = if is_error {
                        "Tool error"
                    } else {
                        "Tool result"
                    };
                    let formatted = format!("\n[{}]\n{}\n", prefix, text);
                    sink.flush(&formatted, false).await?;
                }
                Some(ControllerEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    input,
                }) => {
                    sink.on_permission_request(&request_id, &tool_name, input.as_ref())
                        .await?;
                }
                Some(ControllerEvent::ConfirmRequest {
                    request_id,
                    prompt,
                    options,
                }) => {
                    sink.on_confirm_request(&request_id, &prompt, &options)
                        .await?;
                }
                Some(ControllerEvent::SelectRequest {
                    request_id,
                    prompt,
                    options,
                }) => {
                    sink.on_select_request(&request_id, &prompt, &options)
                        .await?;
                }
                Some(ControllerEvent::QuestionRequest {
                    request_id,
                    questions,
                }) => {
                    sink.on_question_request(&request_id, &questions).await?;
                }
                Some(ControllerEvent::Error(text)) => {
                    sink.flush(&format!("Error: {}", text), false).await?;
                }
                Some(ControllerEvent::Done) | None => {
                    sink.flush("", true).await?;
                    // Drain any late-arriving events (e.g., text sent after Done due to stdout ordering)
                    loop {
                        let late_event = {
                            let mut rx = self.event_rx.lock().await;
                            rx.try_recv().ok()
                        };
                        match late_event {
                            Some(ControllerEvent::Text(text)) => {
                                sink.flush(&text, false).await?;
                            }
                            Some(ControllerEvent::Thinking(text)) => {
                                if text.trim().is_empty() {
                                    if !hidden_thinking_placeholder_sent {
                                        sink.flush(crate::t!("claude.thinking_placeholder"), false)
                                            .await?;
                                        hidden_thinking_placeholder_sent = true;
                                    }
                                } else {
                                    sink.flush(&text, false).await?;
                                }
                            }
                            Some(ControllerEvent::ToolUse(name, input)) => {
                                let text = format!("\n[Tool: {}]\n{}\n", name, input);
                                sink.flush(&text, false).await?;
                            }
                            Some(ControllerEvent::ToolResult(text, is_error)) => {
                                let prefix = if is_error {
                                    "Tool error"
                                } else {
                                    "Tool result"
                                };
                                let formatted = format!("\n[{}]\n{}\n", prefix, text);
                                sink.flush(&formatted, false).await?;
                            }
                            Some(ControllerEvent::Error(text)) => {
                                sink.flush(&format!("Error: {}", text), false).await?;
                            }
                            _ => break,
                        }
                    }
                    break;
                }
            }
        }

        debug!("AgentEventPoller: poll loop ended");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_text_buffer_accumulates_until_take() {
        let mut buf = TurnTextBuffer::default();
        buf.push("line1\n");
        buf.push("line2\n");
        assert_eq!(buf.take_nonempty().as_deref(), Some("line1\nline2\n"));
        assert!(buf.take_nonempty().is_none());
    }

    #[test]
    fn should_flush_on_done_or_error() {
        assert!(!should_flush_turn_buffer("partial", false));
        assert!(should_flush_turn_buffer("", true));
        assert!(should_flush_turn_buffer("Error: boom", false));
    }
}
