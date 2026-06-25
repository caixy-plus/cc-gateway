//! Persistent per-session agent event poller (WebUI + chat platforms).
//!
//! One background task per active agent session loops on [`AgentEventPoller::run_buffered`].
//! User turns only call [`crate::session::channel_manager::ChannelManager::send_to_controller`];
//! the poller keeps listening after each `Done`, so late stream chunks are not stranded.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tracing::{info, warn};

use crate::runtime::controller::AgentController;
use crate::runtime::event_poller::{AgentEventPoller, BufferedSink, EventPollSink};

/// Configuration shared by WebUI, Feishu, and Telegram pollers.
pub struct SessionPollerConfig {
    pub log_label: &'static str,
    pub session_id: String,
    pub flush_interval: std::time::Duration,
    pub max_buffer_chars: usize,
}

/// Called after each agent turn finishes (`run_buffered` returned).
pub type TurnCompleteHook = Arc<dyn Fn(bool) + Send + Sync>;

/// Spawn a long-running poller that survives across user messages until the session stops.
pub fn spawn_session_poller<S, F>(
    controller: Arc<Mutex<AgentController>>,
    config: SessionPollerConfig,
    sink_factory: F,
    on_turn_complete: Option<TurnCompleteHook>,
) -> AbortHandle
where
    S: EventPollSink + Send + 'static,
    F: Fn() -> S + Send + Sync + 'static,
{
    let SessionPollerConfig {
        log_label,
        session_id,
        flush_interval,
        max_buffer_chars,
    } = config;
    let handle = tokio::spawn(async move {
        info!(
            "[{log_label}] Persistent poller started for session {session_id}"
        );
        loop {
            let poller = {
                let ctrl = controller.lock().await;
                if !ctrl.is_session_active().await {
                    info!(
                        "[{log_label}] Session {session_id} inactive, poller exiting"
                    );
                    break;
                }
                AgentEventPoller::from_controller(&ctrl)
            };

            let mut sink = BufferedSink::new(sink_factory(), flush_interval, max_buffer_chars);
            let result = poller.run_buffered(&mut sink).await;
            if let Some(ref hook) = on_turn_complete {
                hook(result.is_ok());
            }
            if let Err(e) = result {
                warn!("[{log_label}] Poller error for session {session_id}: {e}");
            }

            let still_active = {
                let ctrl = controller.lock().await;
                ctrl.is_session_active().await
            };
            if !still_active {
                break;
            }
        }
        info!(
            "[{log_label}] Persistent poller ended for session {session_id}"
        );
    });
    handle.abort_handle()
}

/// Start the poller once; no-op if already running.
pub async fn ensure_poller(
    handle_slot: &Arc<Mutex<Option<AbortHandle>>>,
    spawn: impl FnOnce() -> AbortHandle,
) {
    let mut guard = handle_slot.lock().await;
    if guard.is_some() {
        return;
    }
    *guard = Some(spawn());
}

/// Abort the poller task and clear the stored handle.
pub async fn abort_poller(handle_slot: &Arc<Mutex<Option<AbortHandle>>>) {
    if let Some(handle) = handle_slot.lock().await.take() {
        handle.abort();
    }
}
