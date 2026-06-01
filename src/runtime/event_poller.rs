use anyhow::Result;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::debug;

use crate::runtime::controller::{AgentController, ControllerEvent};

/// After `Done`, keep draining the event channel briefly so late ACP `session/update`
/// chunks (Cursor/OpenCode ACP) are not dropped when the prompt RPC returns early.
const LATE_EVENT_GRACE: Duration = Duration::from_millis(2000);

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

    pub fn char_len(&self) -> usize {
        self.inner.chars().count()
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

#[derive(Clone, Copy, Debug)]
pub struct BufferPolicy {
    pub flush_interval: std::time::Duration,
    pub max_chars: usize,
}

/// Generic buffering wrapper so platforms/UIs don't re-implement flush policy.
pub struct BufferedSink<T: EventPollSink + Send> {
    inner: T,
    text_buffer: TurnTextBuffer,
    policy: BufferPolicy,
    last_flush_at: Option<std::time::Instant>,
}

impl<T: EventPollSink + Send> BufferedSink<T> {
    pub fn new(inner: T, flush_interval: std::time::Duration, max_chars: usize) -> Self {
        Self {
            inner,
            text_buffer: Default::default(),
            policy: BufferPolicy {
                flush_interval,
                max_chars,
            },
            last_flush_at: None,
        }
    }

    #[cfg(test)]
    pub fn into_inner(self) -> T {
        self.inner
    }

    async fn flush_buffer(&mut self, is_done: bool) -> Result<()> {
        if let Some(message) = self.text_buffer.take_nonempty() {
            self.last_flush_at = Some(std::time::Instant::now());
            self.inner.flush(&message, is_done).await?;
        }
        Ok(())
    }

    fn has_pending(&self) -> bool {
        self.text_buffer.char_len() > 0
    }

    fn next_deadline(&self) -> Option<std::time::Instant> {
        if !self.has_pending() {
            return None;
        }
        let last = self
            .last_flush_at
            .unwrap_or_else(|| std::time::Instant::now() - self.policy.flush_interval);
        Some(last + self.policy.flush_interval)
    }
}

/// Sink trait for consuming Claude events during a poll cycle.
/// Each platform/TUI provides its own implementation.
#[async_trait::async_trait]
pub trait EventPollSink: Send {
    /// Called when Claude emits a text/thinking/tool output chunk.
    /// `is_done` is true when the response stream for this turn is complete.
    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()>;

    /// Called when Claude emits a thinking chunk. Platforms can override to send
    /// it immediately (without buffering) to reduce perceived latency.
    async fn on_thinking(&mut self, text: &str) -> Result<()> {
        self.flush(text, false).await
    }

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

#[async_trait::async_trait]
impl<T> EventPollSink for BufferedSink<T>
where
    T: EventPollSink + Send,
{
    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()> {
        self.text_buffer.push(text);
        if should_flush_turn_buffer(text, is_done)
            || self.text_buffer.char_len() >= self.policy.max_chars
        {
            self.flush_buffer(is_done).await?;
        }
        Ok(())
    }

    async fn on_thinking(&mut self, text: &str) -> Result<()> {
        // Preserve ordering: flush any pending assistant text first.
        self.flush_buffer(false).await?;
        self.inner.on_thinking(text).await
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        input: Option<&Value>,
    ) -> Result<()> {
        self.inner
            .on_permission_request(request_id, tool_name, input)
            .await
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        self.inner
            .on_confirm_request(request_id, prompt, options)
            .await
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        self.inner
            .on_select_request(request_id, prompt, options)
            .await
    }

    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::runtime::controller::QuestionItem],
    ) -> Result<()> {
        self.inner.on_question_request(request_id, questions).await
    }
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

    /// Run the poll loop with time-based buffering.
    ///
    /// Guarantees that once any text is buffered, it will be delivered at least
    /// every `policy.flush_interval`, and never exceeds `policy.max_chars` before
    /// being flushed.
    pub async fn run_buffered<T: EventPollSink + Send>(
        self,
        sink: &mut BufferedSink<T>,
    ) -> Result<()> {
        let mut hidden_thinking_placeholder_sent = false;
        loop {
            let deadline = sink.next_deadline();
            let event = match deadline {
                Some(when) => {
                    let mut rx = self.event_rx.lock().await;
                    match tokio::time::timeout_at(tokio::time::Instant::from_std(when), rx.recv())
                        .await
                    {
                        Ok(ev) => ev,
                        Err(_) => {
                            // Timer tick: flush pending buffer.
                            sink.flush_buffer(false).await?;
                            continue;
                        }
                    }
                }
                None => {
                    let mut rx = self.event_rx.lock().await;
                    rx.recv().await
                }
            };

            match event {
                Some(ControllerEvent::Text(text)) => {
                    sink.flush(&text, false).await?;
                }
                Some(ControllerEvent::Thinking(text)) => {
                    if text.trim().is_empty() {
                        if !hidden_thinking_placeholder_sent {
                            sink.on_thinking(crate::t!("claude.thinking_placeholder"))
                                .await?;
                            hidden_thinking_placeholder_sent = true;
                        }
                    } else {
                        sink.on_thinking(&text).await?;
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
                    // Before interactive prompts, flush any pending output so the user
                    // sees context promptly.
                    sink.flush_buffer(false).await?;
                    sink.on_permission_request(&request_id, &tool_name, input.as_ref())
                        .await?;
                }
                Some(ControllerEvent::ConfirmRequest {
                    request_id,
                    prompt,
                    options,
                }) => {
                    sink.flush_buffer(false).await?;
                    sink.on_confirm_request(&request_id, &prompt, &options)
                        .await?;
                }
                Some(ControllerEvent::SelectRequest {
                    request_id,
                    prompt,
                    options,
                }) => {
                    sink.flush_buffer(false).await?;
                    sink.on_select_request(&request_id, &prompt, &options)
                        .await?;
                }
                Some(ControllerEvent::QuestionRequest {
                    request_id,
                    questions,
                }) => {
                    sink.flush_buffer(false).await?;
                    sink.on_question_request(&request_id, &questions).await?;
                }
                Some(ControllerEvent::Error(text)) => {
                    sink.flush(&format!("Error: {}", text), false).await?;
                }
                Some(ControllerEvent::Done) | None => {
                    sink.flush_buffer(true).await?;
                    sink.flush("", true).await?;
                    self.drain_late_events(sink, &mut hidden_thinking_placeholder_sent)
                        .await?;
                    sink.flush_buffer(true).await?;
                    break;
                }
            }
        }

        debug!("AgentEventPoller: buffered poll loop ended");
        Ok(())
    }

    async fn drain_late_events<T: EventPollSink + Send>(
        &self,
        sink: &mut BufferedSink<T>,
        hidden_thinking_placeholder_sent: &mut bool,
    ) -> Result<()> {
        let deadline = std::time::Instant::now() + LATE_EVENT_GRACE;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let late_event = {
                let mut rx = self.event_rx.lock().await;
                match tokio::time::timeout(remaining, rx.recv()).await {
                    Ok(ev) => ev,
                    Err(_) => break,
                }
            };
            match late_event {
                None => break,
                Some(ControllerEvent::Done) => continue,
                Some(ControllerEvent::Text(text)) => {
                    sink.flush(&text, false).await?;
                }
                Some(ControllerEvent::Thinking(text)) => {
                    if text.trim().is_empty() {
                        if !*hidden_thinking_placeholder_sent {
                            sink.on_thinking(crate::t!("claude.thinking_placeholder"))
                                .await?;
                            *hidden_thinking_placeholder_sent = true;
                        }
                    } else {
                        sink.on_thinking(&text).await?;
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
                Some(
                    ControllerEvent::PermissionRequest { .. }
                    | ControllerEvent::ConfirmRequest { .. }
                    | ControllerEvent::SelectRequest { .. }
                    | ControllerEvent::QuestionRequest { .. },
                ) => break,
            }
        }
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
