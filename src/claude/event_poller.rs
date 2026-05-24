use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::claude::controller::{ClaudeController, ControllerEvent};
use crate::claude::event_formatter::EventAccumulator;

/// Default idle timeout in seconds: the poller exits if no event arrives
/// for this duration. As long as events keep flowing, it continues indefinitely.
pub const DEFAULT_DEADLINE_SECS: u64 = 300;
/// Default interval for partial flushing of accumulated output.
pub const DEFAULT_FLUSH_INTERVAL_MILLIS: u64 = 1000;
/// Default threshold for flushing accumulated text after the first chunk.
pub const DEFAULT_FLUSH_THRESHOLD_CHARS: usize = 300;

/// Trait for consumers that want to receive events from a Claude session poll loop.
///
/// Implementors handle platform-specific actions like sending messages,
/// displaying interactive cards, or broadcasting to WebUI clients.
#[async_trait::async_trait]
pub trait EventPollSink: Send {
    /// Called when accumulated text should be flushed to the user.
    /// `is_done` is true when this is the final flush after the session ends.
    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()>;

    /// Called when a permission request is received.
    /// The implementor should display an approval UI and return.
    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        input: Option<&serde_json::Value>,
    ) -> Result<()>;

    /// Called when a confirmation request is received.
    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()>;

    /// Called when a single-select request is received.
    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()>;

    /// Called when a question (multi-step input) request is received.
    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::claude::controller::QuestionItem],
    ) -> Result<()>;
}

/// Configuration for the Claude event poller.
#[derive(Debug, Clone, Copy)]
pub struct PollerConfig {
    /// Total time to wait for the session to complete before forcing exit.
    pub deadline_secs: u64,
    /// How often to flush partial output when no new events arrive.
    pub flush_interval_millis: u64,
    /// Character threshold for flushing after the first text chunk.
    pub flush_threshold_chars: usize,
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            deadline_secs: DEFAULT_DEADLINE_SECS,
            flush_interval_millis: DEFAULT_FLUSH_INTERVAL_MILLIS,
            flush_threshold_chars: DEFAULT_FLUSH_THRESHOLD_CHARS,
        }
    }
}


/// Generic Claude event poller that encapsulates the common polling logic
/// used by Feishu, Telegram, and WebUI consumers.
///
/// Usage:
/// ```ignore
/// let poller = ClaudeEventPoller::new(controller, config);
/// poller.run(&mut my_sink).await?;
/// ```
pub struct ClaudeEventPoller {
    event_rx: Arc<Mutex<tokio::sync::mpsc::UnboundedReceiver<ControllerEvent>>>,
    config: PollerConfig,
}

impl ClaudeEventPoller {
    /// Create a new poller from a controller, cloning its event receiver.
    pub fn from_controller(controller: &ClaudeController) -> Self {
        let event_rx = controller.event_rx_clone();
        Self {
            event_rx,
            config: PollerConfig::default(),
        }
    }


    /// Run the poll loop, forwarding events to the provided sink.
    ///
    /// Uses an idle timeout: the loop exits if no event arrives for
    /// `deadline_secs`. As long as events keep flowing, the poller
    /// continues indefinitely — it does not enforce a wall-clock limit.
    pub async fn run<S: EventPollSink>(self, sink: &mut S) -> Result<()> {
        let mut accumulator = EventAccumulator::new();
        let idle_timeout = Duration::from_secs(self.config.deadline_secs);
        let mut ticker = interval(Duration::from_millis(self.config.flush_interval_millis));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut first_text_sent = false;
        let mut last_event_at = Instant::now();

        loop {
            let idle_elapsed = last_event_at.elapsed();
            let remaining = idle_timeout.saturating_sub(idle_elapsed);
            if remaining.is_zero() {
                break;
            }

            let event_fut = async {
                let mut rx = self.event_rx.lock().await;
                rx.recv().await
            };
            tokio::pin!(event_fut);

            tokio::select! {
                _ = ticker.tick() => {
                    let partial = accumulator.take_output();
                    if !partial.trim().is_empty() {
                        let _ = sink.flush(&partial, false).await;
                    }
                }
                event_res = tokio::time::timeout(remaining, event_fut) => {
                    match event_res {
                        Ok(Some(event)) => {
                            last_event_at = Instant::now();

                            if self.handle_special_event(&event, sink).await? {
                                continue;
                            }

                            let is_text = matches!(event, ControllerEvent::Text(_));
                            let is_done = accumulator.process_event(&event);

                            let should_flush = if !first_text_sent {
                                is_text
                            } else {
                                accumulator.peek_output().len() >= self.config.flush_threshold_chars
                            };

                            if is_text && should_flush {
                                let partial = accumulator.take_output();
                                if !partial.trim().is_empty() {
                                    sink.flush(&partial, false).await?;
                                    first_text_sent = true;
                                }
                            }

                            if is_done {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }

        // Final flush of any remaining accumulated output
        let reply = accumulator.take_output();
        if !reply.trim().is_empty() {
            sink.flush(reply.trim(), true).await?;
        }

        Ok(())
    }

    /// Handle special events that require platform-specific behavior.
    /// Returns `true` if the event was consumed and should not be processed
    /// further by the accumulator.
    async fn handle_special_event<S: EventPollSink>(
        &self,
        event: &ControllerEvent,
        sink: &mut S,
    ) -> Result<bool> {
        match event {
            ControllerEvent::PermissionRequest {
                request_id,
                tool_name,
                input,
            } => {
                sink.on_permission_request(request_id, tool_name, input.as_ref()).await?;
                Ok(true)
            }
            ControllerEvent::ConfirmRequest {
                request_id,
                prompt,
                options,
            } => {
                sink.on_confirm_request(request_id, prompt, options).await?;
                Ok(true)
            }
            ControllerEvent::SelectRequest {
                request_id,
                prompt,
                options,
            } => {
                sink.on_select_request(request_id, prompt, options).await?;
                Ok(true)
            }
            ControllerEvent::QuestionRequest {
                request_id,
                questions,
            } => {
                sink.on_question_request(request_id, questions).await?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

