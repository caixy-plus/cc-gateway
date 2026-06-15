use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::info;

use crate::agent::event::AgentEvent;
pub use crate::agent::event::QuestionItem;
use crate::agent::session::{AgentRuntime, NewProviderSessionCtx};
use crate::command::models;
use crate::config::model::{AgentProfiles, AgentProvider};
use crate::runtime::mcp_server::McpContext;
use crate::runtime::protocol::{build_permission_allow, build_permission_deny, InputMessage};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
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

pub struct AgentController {
    config: AgentProfiles,
    show_thinking: Arc<AtomicBool>,
    session: Arc<RwLock<Option<AgentRuntime>>>,
    event_tx: mpsc::UnboundedSender<ControllerEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>>,
    work_dir: Arc<RwLock<String>>,
    pending_permission: Arc<RwLock<Option<(String, String, String)>>>,
    session_state: Arc<RwLock<SessionState>>,
    message_buffer: Arc<Mutex<Vec<String>>>,
    provider_session_id: Arc<RwLock<Option<String>>>,
    /// Gateway `agent_sessions.id` row to update when the provider session id changes.
    linked_agent_record_id: Arc<RwLock<Option<String>>>,
    active_provider: Arc<RwLock<Option<String>>>,
    pending_resume_session_id: Arc<RwLock<Option<String>>>,
    pending_resume_provider: Arc<RwLock<Option<AgentProvider>>>,
    pending_resume_record_id: Arc<RwLock<Option<String>>>,
    mcp_context: Arc<RwLock<Option<McpContext>>>,
    /// Active model id when known (`--model` at spawn or after `/models` switch).
    active_model_id: Arc<RwLock<Option<String>>>,
    /// Whether the agent is currently generating output (true) or idle/ready (false).
    is_busy: Arc<AtomicBool>,
}

impl AgentController {
    pub fn agent_profiles(&self) -> &AgentProfiles {
        &self.config
    }

    pub fn new(config: AgentProfiles, show_thinking: bool) -> Self {
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
            provider_session_id: Arc::new(RwLock::new(None)),
            linked_agent_record_id: Arc::new(RwLock::new(None)),
            active_provider: Arc::new(RwLock::new(None)),
            pending_resume_session_id: Arc::new(RwLock::new(None)),
            pending_resume_provider: Arc::new(RwLock::new(None)),
            pending_resume_record_id: Arc::new(RwLock::new(None)),
            mcp_context: Arc::new(RwLock::new(None)),
            active_model_id: Arc::new(RwLock::new(None)),
            is_busy: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn init_work_dir(&self, dir: String) {
        let mut wd = self.work_dir.write().await;
        *wd = dir;
    }

    #[cfg(test)]
    pub async fn start_session(&self, work_dir: String, extra_args: Vec<String>) -> Result<()> {
        self.start_session_with_provider(work_dir, extra_args, None)
            .await
    }

    /// Bind this controller to a persisted gateway agent record (`agent_sessions.id`).
    pub async fn set_linked_agent_record_id(&self, id: Option<String>) {
        let mut link = self.linked_agent_record_id.write().await;
        *link = id;
    }

    /// Update in-memory provider session id and persist to the linked agent record when set.
    pub async fn set_provider_session_id(&self, id: Option<String>) {
        {
            let mut sid = self.provider_session_id.write().await;
            *sid = id.clone();
        }
        let record_id = self.linked_agent_record_id.read().await.clone();
        if let Some(record_id) = record_id {
            GLOBAL_CHANNEL_SESSIONS.persist_provider_session_id(&record_id, id);
        }
    }

    /// Persist the current in-memory provider session id (no-op if unchanged or unlinked).
    pub async fn sync_provider_session_id_to_record(&self) {
        let id = self.provider_session_id.read().await.clone();
        let record_id = self.linked_agent_record_id.read().await.clone();
        if let Some(record_id) = record_id {
            GLOBAL_CHANNEL_SESSIONS.persist_provider_session_id(&record_id, id);
        }
    }

    /// Test hook: active session without a provider id yet (late SessionId sync tests).
    #[cfg(test)]
    pub async fn test_set_active_without_provider_session_id(&self) {
        {
            let mut state = self.session_state.write().await;
            *state = SessionState::Active;
        }
        self.set_provider_session_id(None).await;
    }

    /// Test hook: simulate an active session with a provider id.
    #[cfg(test)]
    pub async fn test_set_active_with_provider_session_id(&self, id: String) {
        {
            let mut state = self.session_state.write().await;
            *state = SessionState::Active;
        }
        self.set_provider_session_id(Some(id)).await;
    }

    pub async fn start_session_with_provider(
        &self,
        work_dir: String,
        extra_args: Vec<String>,
        provider: Option<AgentProvider>,
    ) -> Result<()> {
        // Keep the gateway agent record link across the internal stop/restart so
        // provider_session_id (and late SessionId events) persist for WebUI resume.
        let preserved_link = self.linked_agent_record_id.read().await.clone();
        self.stop_session().await?;
        if let Some(id) = preserved_link {
            self.set_linked_agent_record_id(Some(id)).await;
        }

        let provider = if provider.is_some() {
            provider
        } else {
            self.pending_resume_provider.write().await.take()
        };
        let config = self.config.config_for_provider(provider)?;
        let validated = ensure_under_home(&work_dir)?;

        {
            let mut model = self.active_model_id.write().await;
            *model = models::extract_model_from_args(&extra_args);
        }

        let resume_id = {
            let mut pending = self.pending_resume_session_id.write().await;
            pending.take().filter(|id| !id.trim().is_empty())
        };
        let resume_id =
            if crate::command::agents::provider_supports_session_resume(&config.provider) {
                resume_id
            } else {
                None
            };

        let mcp_ctx = {
            let ctx = self.mcp_context.read().await;
            ctx.clone()
        };
        let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (mut session, spawned_provider_session_id) = AgentRuntime::spawn(
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
            let provider_name = config.provider.to_string();
            if stderr.trim().is_empty() {
                return Err(anyhow::anyhow!(
                    "{provider_name} process exited immediately after spawn; no stderr output was captured"
                ));
            }
            return Err(anyhow::anyhow!(
                "{provider_name} process exited immediately after spawn: {stderr}"
            ));
        }

        {
            let mut s = self.session.write().await;
            *s = Some(session);
        }
        self.set_provider_session_id(spawned_provider_session_id.clone())
            .await;
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
        // Drain stale events from the previous session.
        // Use try_lock to avoid deadlocking with concurrent event consumers,
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
        let provider_session_id_arc = self.provider_session_id.clone();
        let linked_agent_record_id = self.linked_agent_record_id.clone();
        let is_busy = self.is_busy.clone();
        tokio::spawn(async move {
            while let Some(event) = agent_rx.recv().await {
                // Track idle/busy: agent becomes idle after a turn finishes.
                if matches!(event, AgentEvent::Done | AgentEvent::Error(_)) {
                    is_busy.store(false, Ordering::Relaxed);
                }
                Self::process_agent_event(
                    &event_tx,
                    &pending_perm,
                    &session_arc,
                    &provider_session_id_arc,
                    &linked_agent_record_id,
                    event,
                    &show_thinking,
                    &permission_policy,
                )
                .await;
            }
            let _ = event_tx.send(ControllerEvent::Done);
            is_busy.store(false, Ordering::Relaxed);

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

        info!("Agent session started in {}", validated);
        Ok(())
    }

    pub async fn stop_session(&self) -> Result<()> {
        self.is_busy.store(false, Ordering::Relaxed);
        // Always transition the controller to inactive, even if the underlying
        // provider stop has issues. Otherwise routers may keep treating the
        // session as active and forward messages into a half-stopped runtime.
        let mut stop_result: Result<()> = Ok(());
        let mut s = self.session.write().await;
        if let Some(session) = s.take() {
            stop_result = session.stop().await;
            if let Err(ref e) = stop_result {
                tracing::warn!("Failed to stop agent session: {}", e);
            } else {
                info!("Claude session stopped");
            }
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
        {
            let mut model = self.active_model_id.write().await;
            *model = None;
        }
        // Unblock any active pollers (platforms/WebUI) waiting for turn completion.
        let _ = self.event_tx.send(ControllerEvent::Done);
        // NOTE: we intentionally keep provider_session_id in memory so that a
        // subsequent /agent can resume the same provider session.
        self.set_linked_agent_record_id(None).await;
        stop_result
    }

    /// Force-stop the active session immediately (used by `/quit`).
    pub async fn force_stop_session(&self) -> Result<()> {
        self.is_busy.store(false, Ordering::Relaxed);
        // Same as stop_session(): ensure we always transition to inactive to
        // prevent subsequent messages from being forwarded.
        let mut stop_result: Result<()> = Ok(());
        let mut s = self.session.write().await;
        if let Some(session) = s.take() {
            stop_result = session.force_stop().await;
            if let Err(ref e) = stop_result {
                tracing::warn!("Failed to force-stop agent session: {}", e);
            } else {
                info!("Agent session force-stopped");
            }
        }
        {
            let mut state = self.session_state.write().await;
            info!(
                "SessionState: {:?} -> Inactive (force_stop_session)",
                *state
            );
            *state = SessionState::Inactive;
        }
        {
            let mut buf = self.message_buffer.lock().await;
            buf.clear();
        }
        {
            let mut model = self.active_model_id.write().await;
            *model = None;
        }
        // Unblock any active pollers that are waiting for turn completion.
        let _ = self.event_tx.send(ControllerEvent::Done);
        self.set_linked_agent_record_id(None).await;
        stop_result
    }

    /// Clear current session context by starting a fresh provider session.
    ///
    /// Stops in-flight generation (best-effort), then calls provider-specific
    /// `session/new` (or equivalent). Updates `provider_session_id` when a new id
    /// is returned. The gateway agent record and subprocess stay alive.
    /// Compact conversation context (Claude `/compact`, Pi RPC `compact`).
    pub async fn compact_session(&self, instructions: Option<&str>) -> Result<String> {
        let _ = self.send_stop_generation().await;

        let provider_name = self.provider_name().await;
        let provider = AgentProvider::parse_str(&provider_name);
        if !crate::command::agents::provider_supports_context_compact(&provider) {
            anyhow::bail!(
                "{}",
                crate::t_fmt!(
                    "builtin.compact_not_supported",
                    NAME = crate::command::agents::provider_display_name(&provider)
                )
            );
        }

        let state = self.session_state.read().await.clone();
        match state {
            SessionState::Inactive | SessionState::Starting => {
                anyhow::bail!("{}", t!("controller.no_active_session"))
            }
            SessionState::Active => {
                let mut s = self.session.write().await;
                if let Some(ref mut session) = *s {
                    session.compact_context(instructions).await
                } else {
                    anyhow::bail!("{}", t!("controller.no_active_session"))
                }
            }
        }
    }

    pub async fn clear_session(&self) -> Result<Option<String>> {
        let _ = self.send_stop_generation().await;

        let work_dir = self.get_work_dir().await;
        let provider_name = self.provider_name().await;
        let provider = AgentProvider::parse_str(&provider_name);
        let config = self.config.config_for_provider(Some(provider))?;
        let mcp_ctx = self.mcp_context.read().await.clone();

        let state = self.session_state.read().await.clone();
        match state {
            SessionState::Inactive | SessionState::Starting => {
                anyhow::bail!("{}", t!("controller.no_active_session"))
            }
            SessionState::Active => {
                let ctx = NewProviderSessionCtx {
                    work_dir,
                    config: &config,
                    extra_args: Vec::new(),
                    mcp_context: mcp_ctx,
                };
                let mut s = self.session.write().await;
                let new_id = if let Some(ref mut session) = *s {
                    session.new_provider_session(&ctx).await?
                } else {
                    anyhow::bail!("{}", t!("controller.no_active_session"))
                };
                if let Some(ref id) = new_id {
                    self.set_provider_session_id(Some(id.clone())).await;
                }
                self.is_busy.store(false, Ordering::Relaxed);
                Ok(new_id)
            }
        }
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
                    self.is_busy.store(true, Ordering::Relaxed);
                    Ok(())
                } else {
                    anyhow::bail!("{}", t!("controller.no_active_session"))
                }
            }
        }
    }

    /// Stop the current generation without killing the session.
    pub async fn send_stop_generation(&self) -> Result<()> {
        let state = self.session_state.read().await.clone();
        match state {
            SessionState::Inactive | SessionState::Starting => {
                anyhow::bail!("{}", t!("controller.no_active_session"))
            }
            SessionState::Active => {
                let mut s = self.session.write().await;
                if let Some(ref mut session) = *s {
                    session.send_stop_generation().await?;
                    self.is_busy.store(false, Ordering::Relaxed);
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

    /// Current model id when known (from spawn args, in-session switch, or Pi `get_state`).
    pub async fn current_model_id(&self) -> Option<String> {
        let provider_name = self.provider_name().await;
        let provider = AgentProvider::parse_str(&provider_name);
        let caps = crate::config::agent_registry::capabilities_for(&provider);
        if caps.active_model_from_session
            || matches!(
                caps.list_models,
                crate::config::agent_registry::ListModelsSource::InSessionRpc
            )
        {
            let mut s = self.session.write().await;
            if let Some(session) = s.as_mut() {
                if let Ok(Some(id)) = session.active_model_id().await {
                    let mut cached = self.active_model_id.write().await;
                    *cached = Some(id.clone());
                    return Some(id);
                }
            }
        }
        self.active_model_id.read().await.clone()
    }

    /// List models available for the current active provider, using provider-specific mechanisms.
    ///
    /// - OpenCode: official CLI (`opencode models`)
    /// - Claude: settings discovery + alias fallback; `/models <alias>` respawns with `--model`
    /// - Codex / Kimi / Gemini ACP: model catalog from `session/new` (`configOptions` or `models`)
    /// - Pi: RPC `get_available_models`
    /// - Cursor: not supported (platform-bound agent)
    pub async fn list_available_models(&self) -> Result<Vec<String>> {
        let provider_name = self.provider_name().await;
        let provider = AgentProvider::parse_str(&provider_name);
        let caps = crate::config::agent_registry::capabilities_for(&provider);
        if caps.platform_bound {
            anyhow::bail!(
                "{}",
                crate::t_fmt!(
                    "models.not_supported_platform_agent",
                    NAME = crate::command::agents::provider_display_name(&provider)
                )
            );
        }
        let config = self.config.config_for_provider(Some(provider.clone()))?;
        let work_dir = self.get_work_dir().await;

        match caps.list_models {
            crate::config::agent_registry::ListModelsSource::NotSupported => Ok(vec![]),
            crate::config::agent_registry::ListModelsSource::Curated => {
                Ok(models::list_discovered_models(&provider, &work_dir))
            }
            crate::config::agent_registry::ListModelsSource::InSessionRpc => {
                let mut s = self.session.write().await;
                if let Some(session) = s.as_mut() {
                    return session.list_available_models_in_session().await;
                }
                Ok(vec![])
            }
            crate::config::agent_registry::ListModelsSource::CliSubcommand => {
                models::list_models_via_cli(&provider, &config).await
            }
        }
    }

    /// Switch model for the current provider.
    ///
    /// - Claude: respawns the session with `--resume <id> --model <id>` (preserves context)
    /// - Pi: RPC `set_model` (no restart)
    /// - OpenCode / Kimi: ACP `session/set_model` (no restart)
    pub async fn switch_model(&self, model_arg: &str) -> Result<String> {
        let provider_name = self.provider_name().await;
        let provider = AgentProvider::parse_str(&provider_name);
        let caps = crate::config::agent_registry::capabilities_for(&provider);
        if caps.platform_bound {
            anyhow::bail!(
                "{}",
                crate::t_fmt!(
                    "models.not_supported_platform_agent",
                    NAME = crate::command::agents::provider_display_name(&provider)
                )
            );
        }
        if !caps.in_session_model_switch {
            anyhow::bail!(
                "{}",
                crate::t_fmt!(
                    "models.not_supported",
                    NAME = crate::command::agents::provider_display_name(&provider)
                )
            );
        }
        let options = self.list_available_models().await?;
        let resolved = models::resolve_model_switch_arg(&provider, model_arg, &options)?;
        let mut s = self.session.write().await;
        if let Some(ref mut session) = *s {
            let canonical = session.set_model(&provider, &resolved).await?;
            drop(s);
            let mut active = self.active_model_id.write().await;
            *active = Some(canonical.clone());
            return Ok(canonical);
        }
        anyhow::bail!("{}", t!("controller.no_active_session"))
    }

    /// Whether the agent is currently generating output (true) or ready for input (false).
    pub fn is_busy(&self) -> bool {
        self.is_busy.load(Ordering::Relaxed)
    }

    /// Summary of current session state, used by `/status`.
    /// Only called when a session is active; returns ready or busy.
    pub async fn status_summary(&self) -> String {
        if self.is_busy() {
            t!("builtin.status_busy").to_string()
        } else {
            t!("builtin.status_ready").to_string()
        }
    }

    /// Clone the internal event receiver Arc so consumers can poll events
    /// without holding the Controller mutex.
    pub fn event_rx_clone(&self) -> Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>> {
        self.event_rx.clone()
    }

    pub fn set_show_thinking(&self, value: bool) {
        self.show_thinking.store(value, Ordering::Relaxed);
    }

    /// Returns (request_id, request_type) of the most recent pending control request, or None.
    pub async fn get_pending_request(&self) -> Option<(String, String)> {
        let p = self.pending_permission.read().await;
        p.as_ref()
            .map(|(id, _name, req_type)| (id.clone(), req_type.clone()))
    }

    /// Clear the pending control request after it has been handled.
    pub async fn clear_pending_request(&self) {
        let mut p = self.pending_permission.write().await;
        *p = None;
    }

    pub async fn get_provider_session_id(&self) -> Option<String> {
        let sid = self.provider_session_id.read().await;
        sid.clone()
    }

    pub async fn provider_name(&self) -> String {
        self.active_provider
            .read()
            .await
            .clone()
            .unwrap_or_else(|| {
                self.config
                    .effective_config()
                    .map(|c| c.provider.to_string())
                    .unwrap_or_else(|_| self.config.default.to_string())
            })
    }

    pub async fn set_pending_resume_session_id(&self, id: Option<String>) {
        let mut sid = self.pending_resume_session_id.write().await;
        *sid = id;
    }

    pub async fn set_pending_resume_provider(&self, provider: Option<AgentProvider>) {
        let mut p = self.pending_resume_provider.write().await;
        *p = provider;
    }

    pub async fn take_pending_resume_provider(&self) -> Option<AgentProvider> {
        self.pending_resume_provider.write().await.take()
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

    #[allow(clippy::too_many_arguments, clippy::type_complexity)]
    async fn process_agent_event(
        event_tx: &mpsc::UnboundedSender<ControllerEvent>,
        pending_perm: &Arc<RwLock<Option<(String, String, String)>>>,
        session_arc: &Arc<RwLock<Option<AgentRuntime>>>,
        provider_session_id: &Arc<RwLock<Option<String>>>,
        linked_agent_record_id: &Arc<RwLock<Option<String>>>,
        event: AgentEvent,
        show_thinking: &Arc<AtomicBool>,
        permission_policy: &str,
    ) {
        match event {
            AgentEvent::SessionId(id) => {
                tracing::debug!("Agent session ID: {}", id);
                {
                    let mut sid = provider_session_id.write().await;
                    *sid = Some(id.clone());
                }
                if let Some(record_id) = linked_agent_record_id.read().await.clone() {
                    GLOBAL_CHANNEL_SESSIONS.persist_provider_session_id(&record_id, Some(id));
                }
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
                if crate::agent::mcp_attach::is_gateway_send_file_tool(&tool_name) {
                    let mut s = session_arc.write().await;
                    if let Some(ref mut session) = *s {
                        let allow_msg = build_permission_allow(&request_id, None);
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
                            build_permission_allow(&request_id, None)
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
                let req_id = request_id.clone();
                let tname = tool_name.clone();
                tokio::spawn(async move {
                    let mut p = pp.write().await;
                    *p = Some((req_id, tname, "permission".to_string()));
                });
            }
            AgentEvent::ConfirmRequest {
                request_id,
                prompt,
                options,
            } => {
                let _ = event_tx.send(ControllerEvent::ConfirmRequest {
                    request_id: request_id.clone(),
                    prompt,
                    options,
                });
                let pp = pending_perm.clone();
                let req_id = request_id;
                tokio::spawn(async move {
                    let mut p = pp.write().await;
                    *p = Some((req_id, "confirm".to_string(), "confirm".to_string()));
                });
            }
            AgentEvent::SelectRequest {
                request_id,
                prompt,
                options,
            } => {
                let _ = event_tx.send(ControllerEvent::SelectRequest {
                    request_id: request_id.clone(),
                    prompt,
                    options,
                });
                let pp = pending_perm.clone();
                let req_id = request_id;
                tokio::spawn(async move {
                    let mut p = pp.write().await;
                    *p = Some((req_id, "select".to_string(), "select".to_string()));
                });
            }
            AgentEvent::QuestionRequest {
                request_id,
                questions,
            } => {
                let _ = event_tx.send(ControllerEvent::QuestionRequest {
                    request_id: request_id.clone(),
                    questions,
                });
                let pp = pending_perm.clone();
                let req_id = request_id;
                tokio::spawn(async move {
                    let mut p = pp.write().await;
                    *p = Some((req_id, "question".to_string(), "question".to_string()));
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
