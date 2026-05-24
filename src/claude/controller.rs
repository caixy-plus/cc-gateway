use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, info};

use crate::claude::mcp_server::McpContext;
use crate::claude::protocol::{build_permission_allow, build_user_message, InputMessage, OutputEvent};
use crate::claude::session::ClaudeSession;
use crate::config::model::ClaudeConfig;
use crate::{t, t_fmt};

/// Validate that a path is allowed.
/// On macOS/Linux: must be under the home directory.
/// On Windows: home directory is always allowed; non-system drives are also allowed.
/// Strip the Windows verbatim path prefix (`\\?\`) so that canonicalized
/// paths can be compared with normal paths.
#[cfg(windows)]
fn strip_verbatim_prefix(path: &std::path::Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        std::path::PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

#[cfg(not(windows))]
fn strip_verbatim_prefix(path: &std::path::Path) -> std::path::PathBuf {
    path.to_path_buf()
}

pub(crate) fn ensure_under_home(path: &str) -> Result<String> {
    let expanded = shellexpand::tilde(path).to_string();
    let path_buf = PathBuf::from(&expanded);
    let canonical = path_buf.canonicalize().unwrap_or(path_buf);
    let home = dirs::home_dir().context("Could not determine home directory")?;

    let canonical_clean = strip_verbatim_prefix(&canonical);
    let home_clean = strip_verbatim_prefix(&home);

    // Home directory is always allowed on all platforms.
    if canonical_clean.starts_with(&home_clean) {
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
    PermissionRequest {
        request_id: String,
        tool_name: String,
        input: Option<serde_json::Value>,
    },
    ConfirmRequest {
        request_id: String,
        prompt: String,
        options: Vec<String>,
    },
    SelectRequest {
        request_id: String,
        prompt: String,
        options: Vec<String>,
    },
    QuestionRequest {
        request_id: String,
        questions: Vec<QuestionItem>,
    },
    Error(String),
    Done,
}

#[derive(Debug, Clone)]
pub struct QuestionItem {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
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
    claude_session_id: Arc<RwLock<Option<String>>>,
    pending_resume_session_id: Arc<RwLock<Option<String>>>,
    mcp_context: Arc<RwLock<Option<McpContext>>>,
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
            claude_session_id: Arc::new(RwLock::new(None)),
            pending_resume_session_id: Arc::new(RwLock::new(None)),
            mcp_context: Arc::new(RwLock::new(None)),
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

        let resume_id = {
            let mut pending = self.pending_resume_session_id.write().await;
            pending.take()
        };

        let mcp_ctx = {
            let ctx = self.mcp_context.read().await;
            ctx.clone()
        };
        let (claude_tx, mut claude_rx) = mpsc::unbounded_channel::<OutputEvent>();
        let (mut session, claude_session_id) = ClaudeSession::spawn(validated.clone(), extra_args, &self.config, claude_tx, resume_id, mcp_ctx).await?;

        // Brief delay to let the child stabilize; if it exits immediately (e.g. invalid --resume)
        // we fail fast instead of pretending the session is active.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        if !session.is_alive() {
            return Err(anyhow::anyhow!("Claude process exited immediately after spawn; check stderr logs for details"));
        }

        {
            let mut s = self.session.write().await;
            *s = Some(session);
        }
        {
            let mut sid = self.claude_session_id.write().await;
            *sid = claude_session_id.clone();
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
        // NOTE: we intentionally keep claude_session_id here so that a
        // subsequent /claude can resume the same Claude session.
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

    /// Send an arbitrary InputMessage (e.g. ControlResponse) to the active session.
    pub async fn send_input(&self, msg: InputMessage) -> Result<()> {
        let state = self.session_state.read().await.clone();
        match state {
            SessionState::Inactive => {
                anyhow::bail!("{}", t!("controller.no_active_session"))
            }
            SessionState::Starting => {
                anyhow::bail!("{}", t!("controller.no_active_session"))
            }
            SessionState::Active => {
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

    pub fn set_show_thinking(&self, value: bool) {
        self.show_thinking.store(value, Ordering::Relaxed);
    }

    pub async fn get_claude_session_id(&self) -> Option<String> {
        let sid = self.claude_session_id.read().await;
        sid.clone()
    }

    pub async fn set_claude_session_id(&self, id: Option<String>) {
        let mut sid = self.claude_session_id.write().await;
        *sid = id;
    }

    pub async fn set_pending_resume_session_id(&self, id: Option<String>) {
        let mut sid = self.pending_resume_session_id.write().await;
        *sid = id;
    }

    pub async fn set_mcp_context(&self, ctx: McpContext) {
        let mut c = self.mcp_context.write().await;
        *c = Some(ctx);
    }

    pub(crate) async fn process_claude_event(
        event_tx: &mpsc::UnboundedSender<ControllerEvent>,
        pending_perm: &Arc<RwLock<Option<(String, String)>>>,
        _work_dir: &str,
        session_arc: &Arc<RwLock<Option<ClaudeSession>>>,
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
                if let Some((req_id, tool_name, input)) = event.is_permission_request() {
                    let subtype = event.extract_control_subtype().unwrap_or_default();

                    let dispatched = if tool_name == "AskUserQuestion" {
                        if let Some(ref val) = input {
                            if let Some(questions) = val.get("questions").and_then(|q| q.as_array()) {
                                let parsed: Vec<QuestionItem> = questions
                                    .iter()
                                    .filter_map(|q| {
                                        let question = q.get("question")?.as_str()?.to_string();
                                        let header = q.get("header")?.as_str()?.to_string();
                                        let multi_select = q.get("multi_select").and_then(|m| m.as_bool()).unwrap_or(false);
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
                                    Some(ControllerEvent::QuestionRequest {
                                        request_id: req_id.clone(),
                                        questions: parsed,
                                    })
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else if subtype == "confirm" {
                        if let Some(ref val) = input {
                            let prompt = val.get("prompt").and_then(|p| p.as_str()).unwrap_or("").to_string();
                            let options: Vec<String> = val
                                .get("options")
                                .and_then(|o| o.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                                .unwrap_or_default();
                            Some(ControllerEvent::ConfirmRequest {
                                request_id: req_id.clone(),
                                prompt,
                                options,
                            })
                        } else {
                            None
                        }
                    } else if subtype == "select_option" {
                        if let Some(ref val) = input {
                            let prompt = val.get("prompt").and_then(|p| p.as_str()).unwrap_or("").to_string();
                            let options: Vec<String> = val
                                .get("options")
                                .and_then(|o| o.as_array())
                                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                                .unwrap_or_default();
                            Some(ControllerEvent::SelectRequest {
                                request_id: req_id.clone(),
                                prompt,
                                options,
                            })
                        } else {
                            None
                        }
                    } else {
                        // Auto-approve MCP send_file tool — no user interaction needed
                        if tool_name == "mcp__cc-gateway__send_file" {
                            let mut s = session_arc.write().await;
                            if let Some(ref mut session) = *s {
                                let allow_msg = build_permission_allow(&req_id);
                                let _ = session.send(allow_msg).await;
                            }
                        }
                        Some(ControllerEvent::PermissionRequest {
                            request_id: req_id.clone(),
                            tool_name: tool_name.clone(),
                            input: input.clone(),
                        })
                    };

                    let ev = dispatched.unwrap_or_else(|| ControllerEvent::PermissionRequest {
                        request_id: req_id.clone(),
                        tool_name: tool_name.clone(),
                        input: input.clone(),
                    });

                    let _ = event_tx.send(ev);
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
