use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::info;

use crate::agent::event::AgentEvent;
pub use crate::agent::event::QuestionItem;
use crate::agent::session::AgentSession;
use crate::claude::mcp_server::McpContext;
use crate::claude::protocol::{build_permission_allow, build_permission_deny, InputMessage};
use crate::config::model::{AgentProvider, AgentSettings};
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

#[cfg(windows)]
fn path_starts_with(path: &std::path::Path, base: &std::path::Path) -> bool {
    fn normalize(path: &std::path::Path) -> String {
        let clean = strip_verbatim_prefix(path);
        clean
            .to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    }

    let path = normalize(path);
    let base = normalize(base);
    path == base || path.starts_with(&format!("{}\\", base))
}

#[cfg(not(windows))]
fn path_starts_with(path: &std::path::Path, base: &std::path::Path) -> bool {
    path.starts_with(base)
}

fn home_dir_for_validation() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    #[cfg(windows)]
    if let Some(home) = std::env::var_os("USERPROFILE").filter(|h| !h.is_empty()) {
        return Ok(PathBuf::from(home));
    }
    dirs::home_dir().context("Could not determine home directory")
}

pub(crate) fn ensure_under_home(path: &str) -> Result<String> {
    let expanded = shellexpand::tilde(path).to_string();
    let path_buf = PathBuf::from(&expanded);
    let canonical = path_buf.canonicalize().unwrap_or(path_buf);
    let home = home_dir_for_validation()?;

    // Home directory is always allowed on all platforms.
    if path_starts_with(&canonical, &home) {
        return Ok(strip_verbatim_prefix(&canonical)
            .to_string_lossy()
            .to_string());
    }

    // On Windows, also allow paths on non-system drives.
    #[cfg(windows)]
    {
        if let Some(system_drive) = get_system_drive() {
            if !is_on_drive(&canonical, &system_drive) {
                return Ok(strip_verbatim_prefix(&canonical)
                    .to_string_lossy()
                    .to_string());
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
    std::env::var("SystemRoot").ok().and_then(|root| {
        let lower = root.to_lowercase();
        // Extract drive letter like "c:" from "C:\Windows"
        lower.get(..2).map(|s| s.to_string())
    })
}

#[cfg(windows)]
fn is_on_drive(path: &std::path::Path, drive: &str) -> bool {
    let path_str = path.to_string_lossy().to_lowercase();
    let drive_lower = drive.to_lowercase();
    path_str.starts_with(&drive_lower) || path_str.starts_with(&format!(r"\\?\{}", drive_lower))
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

#[derive(Debug, Clone, PartialEq)]
enum SessionState {
    Inactive,
    Starting,
    Active,
}

pub struct ClaudeController {
    config: AgentSettings,
    show_thinking: Arc<AtomicBool>,
    session: Arc<RwLock<Option<AgentSession>>>,
    event_tx: mpsc::UnboundedSender<ControllerEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>>,
    work_dir: Arc<RwLock<String>>,
    pending_permission: Arc<RwLock<Option<(String, String)>>>,
    session_state: Arc<RwLock<SessionState>>,
    message_buffer: Arc<Mutex<Vec<String>>>,
    claude_session_id: Arc<RwLock<Option<String>>>,
    active_provider: Arc<RwLock<Option<String>>>,
    pending_resume_session_id: Arc<RwLock<Option<String>>>,
    pending_resume_record_id: Arc<RwLock<Option<String>>>,
    mcp_context: Arc<RwLock<Option<McpContext>>>,
}

impl ClaudeController {
    pub fn new<C: Into<AgentSettings>>(config: C, show_thinking: bool) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            config: config.into(),
            show_thinking: Arc::new(AtomicBool::new(show_thinking)),
            session: Arc::new(RwLock::new(None)),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            work_dir: Arc::new(RwLock::new(String::new())),
            pending_permission: Arc::new(RwLock::new(None)),
            session_state: Arc::new(RwLock::new(SessionState::Inactive)),
            message_buffer: Arc::new(Mutex::new(Vec::new())),
            claude_session_id: Arc::new(RwLock::new(None)),
            active_provider: Arc::new(RwLock::new(None)),
            pending_resume_session_id: Arc::new(RwLock::new(None)),
            pending_resume_record_id: Arc::new(RwLock::new(None)),
            mcp_context: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn init_work_dir(&self, dir: String) {
        let mut wd = self.work_dir.write().await;
        *wd = dir;
    }

    pub async fn start_session(&self, work_dir: String, extra_args: Vec<String>) -> Result<()> {
        self.start_session_with_provider(work_dir, extra_args, None)
            .await
    }

    pub async fn start_session_with_provider(
        &self,
        work_dir: String,
        extra_args: Vec<String>,
        provider: Option<AgentProvider>,
    ) -> Result<()> {
        // Stop existing session if any
        self.stop_session().await?;

        let config = self.config.config_for_provider(provider);
        let validated = ensure_under_home(&work_dir)?;

        let resume_id = {
            let mut pending = self.pending_resume_session_id.write().await;
            pending.take().filter(|id| !id.trim().is_empty())
        };

        let mcp_ctx = {
            let ctx = self.mcp_context.read().await;
            ctx.clone()
        };
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (mut session, claude_session_id) = AgentSession::spawn(
            validated.clone(),
            extra_args,
            &config,
            agent_tx,
            resume_id,
            mcp_ctx,
        )
        .await?;

        // Brief delay to let the child stabilize; if it exits immediately (e.g. invalid --resume)
        // we fail fast instead of pretending the session is active.
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        if !session.is_alive() {
            let stderr = session.recent_stderr();
            if stderr.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "Claude process exited immediately after spawn; no stderr output was captured"
                ));
            }
            return Err(anyhow::anyhow!(
                "Claude process exited immediately after spawn: {}",
                stderr
            ));
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
            let mut provider = self.active_provider.write().await;
            *provider = Some(config.provider.to_string());
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
        {
            let mut provider = self.active_provider.write().await;
            *provider = None;
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
        let session_arc = self.session.clone();
        let session_state = self.session_state.clone();
        let message_buffer = self.message_buffer.clone();
        let show_thinking = self.show_thinking.clone();
        let permission_policy = config.permission.clone();
        let claude_session_id_arc = self.claude_session_id.clone();
        tokio::spawn(async move {
            while let Some(event) = agent_rx.recv().await {
                Self::process_agent_event(
                    &event_tx,
                    &pending_perm,
                    &session_arc,
                    &claude_session_id_arc,
                    event,
                    &show_thinking,
                    &permission_policy,
                )
                .await;
            }
            let _ = event_tx.send(ControllerEvent::Done);

            // Auto-cleanup: stdout closed means the child process exited
            {
                let mut state = session_state.write().await;
                info!(
                    "SessionState: {:?} -> Inactive (event processor ended)",
                    *state
                );
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
                let _ = session.send_message(&msg).await;
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
                let mut s = self.session.write().await;
                if let Some(ref mut session) = *s {
                    session.send_message(text).await?;
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
                    session.send_input(msg).await?;
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
        info!(
            "is_session_active called, state={:?}, result={}",
            *state, active
        );
        active
    }

    pub async fn get_work_dir(&self) -> String {
        let wd = self.work_dir.read().await;
        wd.clone()
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

    pub async fn provider_name(&self) -> String {
        self.active_provider
            .read()
            .await
            .clone()
            .unwrap_or_else(|| self.config.effective_config().provider.to_string())
    }

    pub async fn set_pending_resume_session_id(&self, id: Option<String>) {
        let mut sid = self.pending_resume_session_id.write().await;
        *sid = id;
    }

    pub async fn set_pending_resume_record_id(&self, id: Option<String>) {
        let mut sid = self.pending_resume_record_id.write().await;
        *sid = id;
    }

    pub async fn take_pending_resume_record_id(&self) -> Option<String> {
        self.pending_resume_record_id.write().await.take()
    }

    pub async fn has_pending_resume_session_id(&self) -> bool {
        self.pending_resume_session_id.read().await.is_some()
    }

    pub async fn set_mcp_context(&self, ctx: McpContext) {
        let mut c = self.mcp_context.write().await;
        *c = Some(ctx);
    }

    pub(crate) async fn process_agent_event(
        event_tx: &mpsc::UnboundedSender<ControllerEvent>,
        pending_perm: &Arc<RwLock<Option<(String, String)>>>,
        session_arc: &Arc<RwLock<Option<AgentSession>>>,
        claude_session_id: &Arc<RwLock<Option<String>>>,
        event: AgentEvent,
        show_thinking: &Arc<AtomicBool>,
        permission_policy: &str,
    ) {
        match event {
            AgentEvent::SessionId(id) => {
                tracing::debug!("Agent session ID: {}", id);
                let mut sid = claude_session_id.write().await;
                *sid = Some(id);
            }
            AgentEvent::Text(text) => {
                let _ = event_tx.send(ControllerEvent::Text(text));
            }
            AgentEvent::Thinking(thinking) => {
                let content = if show_thinking.load(Ordering::Relaxed) {
                    thinking
                } else {
                    String::new()
                };
                let _ = event_tx.send(ControllerEvent::Thinking(content));
            }
            AgentEvent::ToolUse(name, input) => {
                let _ = event_tx.send(ControllerEvent::ToolUse(name, input));
            }
            AgentEvent::ToolResult(text, is_error) => {
                let _ = event_tx.send(ControllerEvent::ToolResult(text, is_error));
            }
            AgentEvent::PermissionRequest {
                request_id,
                tool_name,
                input,
            } => {
                // Auto-approve MCP send_file tool — no user interaction needed.
                if tool_name == "mcp__cc-gateway__send_file" {
                    let mut s = session_arc.write().await;
                    if let Some(ref mut session) = *s {
                        let allow_msg = build_permission_allow(&request_id);
                        let _ = session.send_input(allow_msg).await;
                    }
                    if permission_policy == "allow" || permission_policy == "deny" {
                        return;
                    }
                }
                if permission_policy == "allow" || permission_policy == "deny" {
                    let mut s = session_arc.write().await;
                    if let Some(ref mut session) = *s {
                        let msg = if permission_policy == "allow" {
                            build_permission_allow(&request_id)
                        } else {
                            build_permission_deny(&request_id, "Denied by cc-gateway config")
                        };
                        let _ = session.send_input(msg).await;
                    }
                    return;
                }
                let _ = event_tx.send(ControllerEvent::PermissionRequest {
                    request_id: request_id.clone(),
                    tool_name: tool_name.clone(),
                    input,
                });
                let pp = pending_perm.clone();
                tokio::spawn(async move {
                    let mut p = pp.write().await;
                    *p = Some((request_id, tool_name));
                });
            }
            AgentEvent::ConfirmRequest {
                request_id,
                prompt,
                options,
            } => {
                let _ = event_tx.send(ControllerEvent::ConfirmRequest {
                    request_id,
                    prompt,
                    options,
                });
            }
            AgentEvent::SelectRequest {
                request_id,
                prompt,
                options,
            } => {
                let _ = event_tx.send(ControllerEvent::SelectRequest {
                    request_id,
                    prompt,
                    options,
                });
            }
            AgentEvent::QuestionRequest {
                request_id,
                questions,
            } => {
                let _ = event_tx.send(ControllerEvent::QuestionRequest {
                    request_id,
                    questions,
                });
            }
            AgentEvent::Error(error) => {
                let _ = event_tx.send(ControllerEvent::Error(error));
            }
            AgentEvent::Done => {
                let _ = event_tx.send(ControllerEvent::Done);
            }
        }
    }
}
