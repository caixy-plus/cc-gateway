use anyhow::Result;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::debug;

use crate::runtime::controller::{AgentController, ControllerEvent};

/// After `Done`, keep draining the event channel briefly so late ACP `session/update`
/// chunks (Cursor/OpenCode ACP) are not dropped when the prompt RPC returns early.
const LATE_EVENT_GRACE: Duration = Duration::from_millis(2000);

/// Cap tool input/result payload length to avoid flooding chat channels.
///
/// This is independent of per-platform buffer policies; it bounds a single tool
/// message so one huge JSON/result doesn't dominate the turn.
const TOOL_MESSAGE_MAX_CHARS: usize = 100;

/// Default minimum buffered chars before timer-based flush (chat platforms).
pub const DEFAULT_MIN_TIME_FLUSH_CHARS: usize = 160;

fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if max_chars <= 3 {
        return "...".to_string();
    }
    let len = s.chars().count();
    if len <= max_chars {
        return s.to_string();
    }
    let keep = max_chars - 3;
    let prefix: String = s.chars().take(keep).collect();
    format!("{}...", prefix)
}

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

    pub fn as_str(&self) -> &str {
        &self.inner
    }

    pub fn is_effectively_empty(&self) -> bool {
        self.inner.trim().is_empty()
    }

    pub fn take_nonempty(&mut self) -> Option<String> {
        if self.inner.trim().is_empty() {
            self.inner.clear();
            None
        } else {
            Some(std::mem::take(&mut self.inner))
        }
    }

    /// Split off a prefix of at most `byte_len` bytes (UTF-8 safe), leaving the rest buffered.
    pub fn take_prefix_bytes(&mut self, byte_len: usize) -> Option<String> {
        if self.inner.is_empty() || byte_len == 0 {
            return None;
        }
        let mut split = byte_len.min(self.inner.len());
        while split > 0 && !self.inner.is_char_boundary(split) {
            split -= 1;
        }
        if split == 0 {
            return None;
        }
        let prefix = self.inner[..split].to_string();
        self.inner = self.inner[split..].to_string();
        if prefix.trim().is_empty() {
            None
        } else {
            Some(prefix)
        }
    }
}

/// Prefer paragraph breaks, then sentence ends, when timer-flushing partial text.
fn flush_boundary_pos(text: &str) -> usize {
    if let Some(pos) = text.rfind("\n\n") {
        return pos + 2;
    }
    for pat in ["。\n", ".\n", "! \n", "? \n", "。 ", ". ", "! ", "? "] {
        if let Some(pos) = text.rfind(pat) {
            return pos + pat.len();
        }
    }
    text.len()
}

/// Whether buffered assistant text should be delivered to the user now.
pub fn should_flush_turn_buffer(text: &str, is_done: bool) -> bool {
    is_done || text.starts_with("Error:")
}

#[derive(Clone, Copy, Debug)]
pub struct BufferPolicy {
    pub flush_interval: std::time::Duration,
    pub max_chars: usize,
    /// Timer flush only applies once at least this many chars are buffered.
    pub min_time_flush_chars: usize,
}

impl BufferPolicy {
    /// Feishu/Telegram/QQ/WebUI — batch small chunks to reduce message spam.
    pub fn for_chat_platform(flush_interval: std::time::Duration, max_chars: usize) -> Self {
        Self {
            flush_interval,
            max_chars,
            min_time_flush_chars: DEFAULT_MIN_TIME_FLUSH_CHARS,
        }
    }
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
        Self::with_policy(
            inner,
            BufferPolicy::for_chat_platform(flush_interval, max_chars),
        )
    }

    pub fn with_policy(inner: T, policy: BufferPolicy) -> Self {
        Self {
            inner,
            text_buffer: Default::default(),
            policy,
            last_flush_at: None,
        }
    }

    #[cfg(test)]
    pub fn into_inner(self) -> T {
        self.inner
    }

    async fn flush_buffer(&mut self, is_done: bool) -> Result<()> {
        let message = if is_done {
            self.text_buffer.take_nonempty()
        } else if self.text_buffer.is_effectively_empty() {
            None
        } else {
            let content = self.text_buffer.as_str();
            let boundary = flush_boundary_pos(content);
            if boundary >= content.len() {
                self.text_buffer.take_nonempty()
            } else {
                self.text_buffer.take_prefix_bytes(boundary)
            }
        };
        if let Some(message) = message {
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
        // If we only have a small amount of text buffered, wait for more chunks
        // (or Done) instead of flushing on the timer — this reduces "one sentence
        // becomes several messages" for streaming providers.
        if self.text_buffer.char_len() < self.policy.min_time_flush_chars {
            return None;
        }
        let last = self
            .last_flush_at
            .unwrap_or_else(|| std::time::Instant::now() - self.policy.flush_interval);
        Some(last + self.policy.flush_interval)
    }
}

/// Sink trait for consuming Claude events during a poll cycle.
/// Each platform (and WebUI) provides its own implementation.
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
        self.run_buffered_with_grace(sink, LATE_EVENT_GRACE).await
    }

    pub async fn run_buffered_with_grace<T: EventPollSink + Send>(
        self,
        sink: &mut BufferedSink<T>,
        late_event_grace: Duration,
    ) -> Result<()> {
        // We always show a single "Thinking..." marker per turn if the agent emits
        // any thinking content (whether hidden or shown), so all providers/channels
        // have consistent UX.
        let mut thinking_placeholder_sent = false;
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
                        if !thinking_placeholder_sent {
                            sink.on_thinking(crate::t!("builtin.thinking_placeholder"))
                                .await?;
                            thinking_placeholder_sent = true;
                        }
                    } else {
                        if !thinking_placeholder_sent {
                            sink.on_thinking(crate::t!("builtin.thinking_placeholder"))
                                .await?;
                            thinking_placeholder_sent = true;
                        }
                        sink.on_thinking(&text).await?;
                    }
                }
                Some(ControllerEvent::ToolUse(name, input)) => {
                    let input = truncate_with_ellipsis(&input, TOOL_MESSAGE_MAX_CHARS);
                    let text = format!("\n[Tool: {}]\n{}\n", name, input);
                    sink.flush(&text, false).await?;
                }
                Some(ControllerEvent::ToolResult(text, is_error)) => {
                    let prefix = if is_error {
                        "Tool error"
                    } else {
                        "Tool result"
                    };
                    let text = truncate_with_ellipsis(&text, TOOL_MESSAGE_MAX_CHARS);
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
                    self.drain_late_events(sink, &mut thinking_placeholder_sent, late_event_grace)
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
        thinking_placeholder_sent: &mut bool,
        late_event_grace: Duration,
    ) -> Result<()> {
        let deadline = std::time::Instant::now() + late_event_grace;
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
                        if !*thinking_placeholder_sent {
                            sink.on_thinking(crate::t!("builtin.thinking_placeholder"))
                                .await?;
                            *thinking_placeholder_sent = true;
                        }
                    } else {
                        if !*thinking_placeholder_sent {
                            sink.on_thinking(crate::t!("builtin.thinking_placeholder"))
                                .await?;
                            *thinking_placeholder_sent = true;
                        }
                        sink.on_thinking(&text).await?;
                    }
                }
                Some(ControllerEvent::ToolUse(name, input)) => {
                    let input = truncate_with_ellipsis(&input, TOOL_MESSAGE_MAX_CHARS);
                    let text = format!("\n[Tool: {}]\n{}\n", name, input);
                    sink.flush(&text, false).await?;
                }
                Some(ControllerEvent::ToolResult(text, is_error)) => {
                    let prefix = if is_error {
                        "Tool error"
                    } else {
                        "Tool result"
                    };
                    let text = truncate_with_ellipsis(&text, TOOL_MESSAGE_MAX_CHARS);
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

    #[test]
    fn flush_boundary_prefers_paragraph_then_sentence() {
        let text = "First para.\n\nSecond starts here and runs on.";
        assert_eq!(flush_boundary_pos(text), "First para.\n\n".len());

        let text = "Hello world. More text without newline break.";
        assert_eq!(flush_boundary_pos(text), "Hello world. ".len());

        let no_boundary = "short chunk no period";
        assert_eq!(flush_boundary_pos(no_boundary), no_boundary.len());
    }

    #[test]
    fn turn_text_buffer_take_prefix_bytes_splits_utf8_safe() {
        let mut buf = TurnTextBuffer::default();
        buf.push("段落一。\n\n段落二继续");
        let first = buf.take_prefix_bytes(flush_boundary_pos(buf.as_str())).unwrap();
        assert_eq!(first, "段落一。\n\n");
        assert_eq!(buf.inner, "段落二继续");
    }
}
