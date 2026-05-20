use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info};

use crate::claude::protocol::{build_user_message, OutputEvent};
use crate::claude::session::ClaudeSession;
use crate::config::model::ClaudeConfig;
use crate::{t, t_fmt};

/// Validate that a path is allowed.
/// On macOS/Linux: must be under the home directory.
/// On Windows: home directory is always allowed; non-system drives are also allowed.
pub(crate) fn ensure_under_home(path: &str) -> Result<String> {
    let expanded = shellexpand::tilde(path).to_string();
    let path_buf = PathBuf::from(&expanded);
    let canonical = path_buf.canonicalize().unwrap_or(path_buf);
    let home = dirs::home_dir().context("Could not determine home directory")?;

    // Home directory is always allowed on all platforms.
    if canonical.starts_with(&home) {
        return Ok(canonical.to_string_lossy().to_string());
    }

    // On Windows, also allow paths on non-system drives.
    #[cfg(windows)]
    {
        if let Some(system_drive) = get_system_drive() {
            if !is_on_drive(&canonical, &system_drive) {
                return Ok(canonical.to_string_lossy().to_string());
            }
        }
    }

    anyhow::bail!(
        "{}",
        t_fmt!(
            "controller.access_denied",
            PATH = canonical.display(),
            HOME = home.display()
        )
    )
}

#[cfg(windows)]
fn get_system_drive() -> Option<String> {
    std::env::var("SystemRoot")
        .ok()
        .and_then(|root| {
            let lower = root.to_lowercase();
            // Extract drive letter like "c:" from "C:\Windows"
            lower.get(..2).map(|s| s.to_string())
        })
}

#[cfg(windows)]
fn is_on_drive(path: &std::path::Path, drive: &str) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    let drive_lower = drive.to_lowercase();
    path_str.starts_with(&drive_lower)
        || path_str.starts_with(&format!(r"\\?\{}", drive_lower))
}

#[derive(Debug, Clone)]
pub enum ControllerEvent {
    Text(String),
    Thinking(String),
    ToolUse(String, String),
    ToolResult(String, bool),
    PermissionRequest(String, String),
    Error(String),
    Done,
}

#[derive(Debug, Clone, PartialEq)]
enum SessionState {
    Inactive,
    Starting,
    Active,
}

pub struct ClaudeController {
    config: ClaudeConfig,
    show_thinking: Arc<AtomicBool>,
    session: Arc<RwLock<Option<ClaudeSession>>>,
    event_tx: mpsc::UnboundedSender<ControllerEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>>,
    work_dir: Arc<RwLock<String>>,
    pending_permission: Arc<RwLock<Option<(String, String)>>>,
    session_state: Arc<RwLock<SessionState>>,
    message_buffer: Arc<Mutex<Vec<String>>>,
}

impl ClaudeController {
    pub fn new(config: ClaudeConfig, show_thinking: bool) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            config,
            show_thinking: Arc::new(AtomicBool::new(show_thinking)),
            session: Arc::new(RwLock::new(None)),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            work_dir: Arc::new(RwLock::new(String::new())),
            pending_permission: Arc::new(RwLock::new(None)),
            session_state: Arc::new(RwLock::new(SessionState::Inactive)),
            message_buffer: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub async fn init_work_dir(&self, dir: String) {
        let mut wd = self.work_dir.write().await;
        *wd = dir;
    }

    pub async fn start_session(&self, work_dir: String, extra_args: Vec<String>) -> Result<()> {
        // Stop existing session if any
        self.stop_session().await?;

        let validated = ensure_under_home(&work_dir)?;

        let (claude_tx, mut claude_rx) = mpsc::unbounded_channel::<OutputEvent>();
        let session = ClaudeSession::spawn(validated.clone(), extra_args, &self.config, claude_tx).await?;

        {
            let mut s = self.session.write().await;
            *s = Some(session);
        }
        {
            let mut wd = self.work_dir.write().await;
            *wd = validated.clone();
        }
        {
            let mut state = self.session_state.write().await;
            *state = SessionState::Starting;
        }
        {
            let mut buf = self.message_buffer.lock().await;
            buf.clear();
        }
        // Drain stale events from the previous session.
        // Use try_lock to avoid deadlocking with the TUI event listener,
        // which holds this lock while waiting on recv() from a non-existent
        // session. If the lock is contended, skip the drain — it's best-effort.
        if let Ok(mut rx) = self.event_rx.try_lock() {
            while rx.try_recv().is_ok() {}
        }

        // Spawn event processor
        let event_tx = self.event_tx.clone();
        let pending_perm = self.pending_permission.clone();
        let work_dir = validated.clone();
        let session_arc = self.session.clone();
        let session_state = self.session_state.clone();
        let message_buffer = self.message_buffer.clone();
        let show_thinking = self.show_thinking.clone();
        tokio::spawn(async move {
            while let Some(event) = claude_rx.recv().await {
                Self::process_claude_event(
                    &event_tx,
                    &pending_perm,
                    &work_dir,
                    &session_arc,
                    event,
                    &show_thinking,
                )
                .await;
            }
            let _ = event_tx.send(ControllerEvent::Done);

            // Auto-cleanup: stdout closed means the child process exited
            {
                let mut state = session_state.write().await;
                info!("SessionState: {:?} -> Inactive (event processor ended)", *state);
                *state = SessionState::Inactive;
            }
            {
                let mut s = session_arc.write().await;
                if s.is_some() {
                    info!("Clearing dead session reference");
                    *s = None;
                }
            }
            {
                let mut buf = message_buffer.lock().await;
                buf.clear();
            }
        });

        // Mark active immediately: the child process has spawned and stdin/stdout
        // pipes are open. Waiting for the first stdout event deadlocks because
        // Claude stream-json mode may not emit anything until it receives input.
        {
            let mut state = self.session_state.write().await;
            info!("SessionState: {:?} -> Active (start_session)", *state);
            *state = SessionState::Active;
        }
        let messages = {
            let mut buf = self.message_buffer.lock().await;
            std::mem::take(&mut *buf)
        };
        for msg in messages {
            let mut s = self.session.write().await;
            if let Some(ref mut session) = *s {
                let user_msg = build_user_message(&msg);
                let _ = session.send(user_msg).await;
            }
            drop(s);
        }

        info!("Claude session started in {}", validated);
        Ok(())
    }

    pub async fn stop_session(&self) -> Result<()> {
        let mut s = self.session.write().await;
        if let Some(session) = s.take() {
            session.stop().await?;
            info!("Claude session stopped");
        }
        {
            let mut state = self.session_state.write().await;
            info!("SessionState: {:?} -> Inactive (stop_session)", *state);
            *state = SessionState::Inactive;
        }
        {
            let mut buf = self.message_buffer.lock().await;
            buf.clear();
        }
        Ok(())
    }

    pub async fn send_message(&self, text: &str) -> Result<()> {
        let state = self.session_state.read().await.clone();
        match state {
            SessionState::Inactive => {
                anyhow::bail!("{}", t!("controller.no_active_session"))
            }
            SessionState::Starting => {
                let mut buf = self.message_buffer.lock().await;
                buf.push(text.to_string());
                Ok(())
            }
            SessionState::Active => {
                let msg = build_user_message(text);
                let mut s = self.session.write().await;
                if let Some(ref mut session) = *s {
                    session.send(msg).await?;
                    Ok(())
                } else {
                    anyhow::bail!("{}", t!("controller.no_active_session"))
                }
            }
        }
    }

    pub async fn is_session_active(&self) -> bool {
        let state = self.session_state.read().await;
        let active = *state != SessionState::Inactive;
        info!("is_session_active called, state={:?}, result={}", *state, active);
        active
    }

    #[cfg(test)]
    pub async fn inject_dummy_session(&self) -> Result<()> {
        let session = crate::claude::session::ClaudeSession::dummy_for_test().await?;
        let mut s = self.session.write().await;
        *s = Some(session);
        let mut state = self.session_state.write().await;
        *state = SessionState::Active;
        Ok(())
    }

    pub async fn get_work_dir(&self) -> String {
        let wd = self.work_dir.read().await;
        wd.clone()
    }

    #[allow(dead_code)]
    pub async fn set_work_dir(&self, dir: String) -> Result<()> {
        let validated = ensure_under_home(&dir)?;
        {
            let mut wd = self.work_dir.write().await;
            *wd = validated.clone();
        }
        // Restart session with new work dir
        self.start_session(validated, vec![]).await?;
        Ok(())
    }

    /// Clone the internal event receiver Arc so consumers can poll events
    /// without holding the Controller mutex.
    pub fn event_rx_clone(&self) -> Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>> {
        self.event_rx.clone()
    }

    pub fn get_show_thinking(&self) -> bool {
        self.show_thinking.load(Ordering::Relaxed)
    }

    pub fn set_show_thinking(&self, value: bool) {
        self.show_thinking.store(value, Ordering::Relaxed);
    }

    pub async fn recv_event(&self) -> Option<ControllerEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    async fn process_claude_event(
        event_tx: &mpsc::UnboundedSender<ControllerEvent>,
        pending_perm: &Arc<RwLock<Option<(String, String)>>>,
        _work_dir: &str,
        _session_arc: &Arc<RwLock<Option<ClaudeSession>>>,
        event: OutputEvent,
        show_thinking: &Arc<AtomicBool>,
    ) {
        match &event {
            OutputEvent::System { session_id } => {
                if let Some(id) = session_id {
                    debug!("Claude session ID: {}", id);
                }
            }
            OutputEvent::Assistant { message } => {
                for block in &message.content {
                    match block {
                        crate::claude::protocol::ContentBlock::Text { text } => {
                            let _ = event_tx.send(ControllerEvent::Text(text.clone()));
                        }
                        crate::claude::protocol::ContentBlock::Thinking { thinking } => {
                            let content = if show_thinking.load(Ordering::Relaxed) {
                                thinking.clone()
                            } else {
                                String::new()
                            };
                            let _ = event_tx.send(ControllerEvent::Thinking(content));
                        }
                        crate::claude::protocol::ContentBlock::ToolUse { name, input } => {
                            let input_str = serde_json::to_string(input).unwrap_or_default();
                            let _ = event_tx.send(ControllerEvent::ToolUse(name.clone(), input_str));
                        }
                        crate::claude::protocol::ContentBlock::ToolResult { content, is_error } => {
                            let text = content.clone().unwrap_or_default();
                            let _ = event_tx.send(ControllerEvent::ToolResult(text, *is_error));
                        }
                        crate::claude::protocol::ContentBlock::Image { source } => {
                            let text = format!(
                                "[Image: {} {} ({} bytes)]",
                                source.source_type, source.media_type, source.data.len()
                            );
                            let _ = event_tx.send(ControllerEvent::Text(text));
                        }
                    }
                }
            }
            OutputEvent::Result { result: _result, usage } => {
                // The result text is the complete assembled response, but we already
                // streamed it via Assistant::Text blocks. Emitting it again here would
                // duplicate the content in the accumulator, causing Feishu users to
                // receive every answer twice.
                if let Some(u) = usage {
                    debug!(
                        "Usage: input={} output={}",
                        u.input_tokens.unwrap_or(0),
                        u.output_tokens.unwrap_or(0)
                    );
                }
                let _ = event_tx.send(ControllerEvent::Done);
            }
            OutputEvent::ControlRequest { .. } => {
                if let Some((req_id, tool_name, _input)) = event.is_permission_request() {
                    let _ = event_tx
                        .send(ControllerEvent::PermissionRequest(req_id.clone(), tool_name.clone()));
                    let pp = pending_perm.clone();
                    let req_id = req_id.clone();
                    let tool_name = tool_name.clone();
                    tokio::spawn(async move {
                        let mut p = pp.write().await;
                        *p = Some((req_id, tool_name));
                    });
                }
            }
            OutputEvent::Error { error } => {
                let _ = event_tx.send(ControllerEvent::Error(error.clone()));
            }
            OutputEvent::User { .. } => {
                // Ignore user echo events
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_init_work_dir_sets_internal_state() {
        let config = ClaudeConfig::default();
        let controller = ClaudeController::new(config, false);
        controller.init_work_dir("/test/path".to_string()).await;
        assert_eq!(controller.get_work_dir().await, "/test/path");
    }

    #[test]
    fn test_ensure_under_home_allows_home_subdir() {
        let home = dirs::home_dir().unwrap();
        let test_path = home.join("some_subdir");
        let result = ensure_under_home(test_path.to_str().unwrap());
        assert!(result.is_ok(), "Should allow path under home: {:?}", result.err());
        let resolved = result.unwrap();
        assert!(resolved.contains("some_subdir"));
    }

    #[test]
    fn test_ensure_under_home_denies_outside_home() {
        let result = ensure_under_home("/tmp");
        assert!(result.is_err(), "Should deny path outside home");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Access denied"), "Error should mention access denied: {}", err);
        assert!(err.contains("outside home directory"), "Error should mention outside home: {}", err);
    }

    #[test]
    fn test_ensure_under_home_denies_root() {
        let result = ensure_under_home("/");
        assert!(result.is_err(), "Should deny root directory");
    }

    #[tokio::test]
    async fn test_start_session_outside_home_denied() {
        let config = ClaudeConfig::default();
        let controller = ClaudeController::new(config, false);
        let result = controller.start_session("/tmp".to_string(), vec![]).await;
        assert!(result.is_err(), "Should deny starting session outside home");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Access denied"), "Error should mention access denied: {}", err);
        assert!(err.contains("outside home directory"), "Error should mention outside home: {}", err);
    }

    #[tokio::test]
    async fn test_set_work_dir_outside_home_denied() {
        let config = ClaudeConfig::default();
        let controller = ClaudeController::new(config, false);
        let result = controller.set_work_dir("/tmp".to_string()).await;
        assert!(result.is_err(), "Should deny changing work dir outside home");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Access denied"), "Error should mention access denied: {}", err);
        assert!(err.contains("outside home directory"), "Error should mention outside home: {}", err);
    }

    #[tokio::test]
    async fn test_set_work_dir_under_home_allowed() {
        let config = ClaudeConfig::default();
        let controller = ClaudeController::new(config, false);
        let home = dirs::home_dir().unwrap();
        let test_dir = home.join("cc_gateway_test_nonexistent_12345");
        let result = controller.set_work_dir(test_dir.to_string_lossy().to_string()).await;
        // Validation should pass; start_session may fail due to missing Claude binary,
        // but it must NOT fail due to home directory restriction.
        if result.is_err() {
            let err = result.unwrap_err().to_string();
            assert!(
                !err.contains("outside home directory"),
                "Should not fail home check: {}",
                err
            );
        }
    }

    #[tokio::test]
    async fn test_process_claude_event_emits_tool_result() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pending_perm = Arc::new(RwLock::new(None));
        let session_arc = Arc::new(RwLock::new(None));

        let event = OutputEvent::Assistant {
            message: crate::claude::protocol::AssistantMessage {
                role: "assistant".to_string(),
                content: vec![
                    crate::claude::protocol::ContentBlock::ToolResult {
                        content: Some("hello output".to_string()),
                        is_error: false,
                    },
                ],
            },
        };
        ClaudeController::process_claude_event(
            &tx,
            &pending_perm,
            "/tmp",
            &session_arc,
            event,
            &Arc::new(AtomicBool::new(true)),
        )
        .await;

        let received = rx.recv().await;
        assert!(matches!(received, Some(ControllerEvent::ToolResult(content, false)) if content == "hello output"));
    }

    #[tokio::test]
    async fn test_process_claude_event_emits_tool_error() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pending_perm = Arc::new(RwLock::new(None));
        let session_arc = Arc::new(RwLock::new(None));

        let event = OutputEvent::Assistant {
            message: crate::claude::protocol::AssistantMessage {
                role: "assistant".to_string(),
                content: vec![
                    crate::claude::protocol::ContentBlock::ToolResult {
                        content: Some("command failed".to_string()),
                        is_error: true,
                    },
                ],
            },
        };
        ClaudeController::process_claude_event(
            &tx,
            &pending_perm,
            "/tmp",
            &session_arc,
            event,
            &Arc::new(AtomicBool::new(true)),
        )
        .await;

        let received = rx.recv().await;
        assert!(matches!(received, Some(ControllerEvent::ToolResult(content, true)) if content == "command failed"));
    }
}
