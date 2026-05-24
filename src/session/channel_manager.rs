use std::path::PathBuf;
use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::claude::controller::ClaudeController;
use crate::claude::mcp_server::McpContext;
use crate::command::router::CommandRouter;
use crate::config::model::ClaudeConfig;
use crate::session::channel_model::{
    ChannelSession, ClaudeSession, ClaudeSessionState,
};

/// Runtime state for an active ClaudeSession within a Channel.
#[derive(Clone)]
pub struct ActiveClaudeRuntime {
    pub claude_session: ClaudeSession,
    pub controller: Arc<Mutex<ClaudeController>>,
    pub router: Arc<CommandRouter>,
}

/// Runtime state for a WebUI channel.
#[derive(Clone)]
pub struct WebUIChannelRuntime {
    pub channel_session: ChannelSession,
    pub active_claude: Option<ActiveClaudeRuntime>,
    /// AbortHandle for the current poll_claude_and_broadcast task.
    /// Used to cancel the previous poller before spawning a new one
    /// to prevent concurrent pollers from splitting the event stream.
    pub poll_abort_handle: Option<tokio::task::AbortHandle>,
}

#[derive(Clone)]
pub struct ChannelSessionManager {
    channel_sessions: Arc<DashMap<String, ChannelSession>>,
    claude_sessions: Arc<DashMap<String, ClaudeSession>>,
    webui_runtimes: Arc<DashMap<String, WebUIChannelRuntime>>,
    /// Per-(platform, channel_id) mutexes to prevent TOCTOU races in
    /// `get_or_create_platform_channel` when two callers arrive concurrently.
    creation_locks: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

pub static GLOBAL_CHANNEL_SESSIONS: Lazy<ChannelSessionManager> =
    Lazy::new(ChannelSessionManager::new);

impl ChannelSessionManager {
    pub fn new() -> Self {
        Self {
            channel_sessions: Arc::new(DashMap::new()),
            claude_sessions: Arc::new(DashMap::new()),
            webui_runtimes: Arc::new(DashMap::new()),
            creation_locks: Arc::new(DashMap::new()),
        }
    }

    pub fn insert_channel(&self, channel: ChannelSession) {
        crate::db::insert_channel_session(&channel);
        self.channel_sessions.insert(channel.id.clone(), channel);
    }

    pub fn get_channel(&self, id: &str) -> Option<ChannelSession> {
        self.channel_sessions.get(id).map(|c| c.clone())
    }

    pub fn list_channels(&self) -> Vec<ChannelSession> {
        self.channel_sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn update_channel_work_dir(&self, id: &str, work_dir: &str) {
        crate::db::update_channel_work_dir(id, work_dir);
        if let Some(mut channel) = self.channel_sessions.get_mut(id) {
            channel.work_dir = work_dir.to_string();
        }
    }

    pub async fn get_or_create_platform_channel(
        &self,
        platform: &str,
        channel_id: &str,
        work_dir: &str,
    ) -> ChannelSession {
        let key = format!("{}:{}", platform, channel_id);
        let lock = self
            .creation_locks
            .entry(key.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        // Re-check after acquiring the lock in case another caller just inserted
        for entry in self.channel_sessions.iter() {
            let c = entry.value();
            if c.platform == platform && c.channel_id == channel_id {
                return c.clone();
            }
        }
        let channel = ChannelSession::new_platform(platform, channel_id, work_dir);
        self.insert_channel(channel.clone());
        channel
    }

    pub fn insert_claude_session(&self, session: ClaudeSession) {
        crate::db::insert_claude_session(&session);
        self.claude_sessions.insert(session.id.clone(), session);
    }

    pub fn get_claude_session(&self, id: &str) -> Option<ClaudeSession> {
        self.claude_sessions.get(id).map(|s| s.clone())
    }

    pub fn remove_claude_session(&self, id: &str) {
        crate::db::delete_claude_session(id);
        self.claude_sessions.remove(id);
    }

    pub fn list_claude_sessions(&self) -> Vec<ClaudeSession> {
        self.claude_sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn list_claude_sessions_by_channel(&self, channel_id: &str) -> Vec<ClaudeSession> {
        let mut sessions: Vec<ClaudeSession> = self
            .claude_sessions
            .iter()
            .filter(|entry| entry.value().channel_session_id == channel_id)
            .map(|entry| entry.value().clone())
            .collect();
        sessions.sort_by(|a, b| {
            let a_time = a.updated_at.unwrap_or(a.created_at);
            let b_time = b.updated_at.unwrap_or(b.created_at);
            b_time.cmp(&a_time)
        });
        sessions.truncate(10);
        sessions
    }

    pub fn touch_claude_session(&self, id: &str) {
        let now = Some(chrono::Utc::now());
        crate::db::update_claude_session_updated_at(id, now);
        if let Some(mut session) = self.claude_sessions.get_mut(id) {
            session.updated_at = now;
        }
    }

    pub fn update_claude_session_state(&self, id: &str, state: ClaudeSessionState) {
        crate::db::update_claude_session_state(id, &state.to_string());
        if let Some(mut session) = self.claude_sessions.get_mut(id) {
            session.state = state;
        }
    }

    pub fn update_claude_session_active(&self, id: &str, active: bool) {
        crate::db::update_claude_session_active(id, active);
        if let Some(mut session) = self.claude_sessions.get_mut(id) {
            session.active = active;
        }
    }

    pub fn update_claude_session_stopped_at(&self, id: &str, stopped_at: Option<chrono::DateTime<chrono::Utc>>) {
        crate::db::update_claude_session_stopped_at(id, stopped_at);
        if let Some(mut session) = self.claude_sessions.get_mut(id) {
            session.stopped_at = stopped_at;
        }
    }

    pub fn mark_claude_session_dead(&self, id: &str) {
        warn!("Marking ClaudeSession {} as Dead", id);
        self.update_claude_session_state(id, ClaudeSessionState::Dead);
        self.update_claude_session_active(id, false);
        self.update_claude_session_stopped_at(id, Some(chrono::Utc::now()));
    }

    pub fn create_claude_session_only(
        &self,
        channel_id: &str,
        title: &str,
        work_dir: &str,
    ) -> anyhow::Result<ClaudeSession> {
        let channel = self
            .get_channel(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;

        // Update channel work_dir when user explicitly selects one
        if !work_dir.is_empty() && work_dir != channel.work_dir {
            self.update_channel_work_dir(channel_id, work_dir);
        }

        let effective_work_dir = if work_dir.is_empty() {
            channel.work_dir.clone()
        } else {
            work_dir.to_string()
        };

        let claude_session = ClaudeSession::new(channel_id, title, &effective_work_dir);
        self.insert_claude_session(claude_session.clone());

        info!(
            "Created ClaudeSession {} in channel {} (work_dir: {}, not started)",
            claude_session.id, channel_id, claude_session.work_dir
        );
        Ok(claude_session)
    }

    /// Shared implementation for starting (or resuming) a Claude session.
    async fn do_start_claude_session(
        &self,
        channel_id: &str,
        controller: ClaudeController,
        mut claude_session: ClaudeSession,
        resume: bool,
        extra_args: Vec<String>,
    ) -> anyhow::Result<(ClaudeSession, Arc<Mutex<ClaudeController>>)> {
        self.stop_active_claude_session(channel_id).await?;

        let controller = Arc::new(Mutex::new(controller));

        {
            let ctrl = controller.lock().await;
            ctrl.init_work_dir(claude_session.work_dir.clone()).await;
            if resume {
                if let Some(ref csid) = claude_session.claude_session_id {
                    ctrl.set_claude_session_id(Some(csid.clone())).await;
                    ctrl.set_pending_resume_session_id(Some(csid.clone())).await;
                }
            }
        }

        let start_result = {
            let ctrl = controller.lock().await;
            ctrl.start_session(claude_session.work_dir.clone(), extra_args.clone())
                .await
        };

        match start_result {
            Ok(()) => {
                let csid = { controller.lock().await.get_claude_session_id().await };
                claude_session.claude_session_id = csid.clone();
                claude_session.active = true;
                claude_session.state = ClaudeSessionState::Active;
                if resume {
                    claude_session.stopped_at = None;
                }
                self.insert_claude_session(claude_session.clone());

                info!(
                    "Started ClaudeSession {} in channel {} (work_dir: {}, resume: {})",
                    claude_session.id, channel_id, claude_session.work_dir, resume
                );
                Ok((claude_session, controller))
            }
            Err(e) => {
                if resume {
                    // Fallback: try a fresh start without resume.
                    // pending_resume_session_id was already taken() by start_session,
                    // so a retry will spawn without --resume.
                    let retry_result = {
                        let ctrl = controller.lock().await;
                        ctrl.start_session(claude_session.work_dir.clone(), extra_args)
                            .await
                    };
                    match retry_result {
                        Ok(()) => {
                            let csid = { controller.lock().await.get_claude_session_id().await };
                            claude_session.claude_session_id = csid.clone();
                            claude_session.active = true;
                            claude_session.state = ClaudeSessionState::Active;
                            claude_session.stopped_at = None;
                            self.insert_claude_session(claude_session.clone());

                            info!(
                                "Started ClaudeSession {} in channel {} (work_dir: {}, resume: false, fallback)",
                                claude_session.id, channel_id, claude_session.work_dir
                            );
                            Ok((claude_session, controller))
                        }
                        Err(e2) => {
                            self.mark_claude_session_dead(&claude_session.id);
                            anyhow::bail!(
                                "Failed to resume session and fresh start also failed. It has been marked as dead. Error: resume={} fresh={}",
                                e, e2
                            )
                        }
                    }
                } else {
                    anyhow::bail!("Failed to start Claude session: {}", e)
                }
            }
        }
    }

    pub async fn create_and_start_claude_session(
        &self,
        channel_id: &str,
        title: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
        extra_args: Vec<String>,
        resume_session_id: Option<String>,
        mcp_context: Option<McpContext>,
    ) -> anyhow::Result<(ClaudeSession, Arc<Mutex<ClaudeController>>)> {
        let channel = self
            .get_channel(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;

        let claude_session = ClaudeSession::new(channel_id, title, &channel.work_dir);

        let controller = ClaudeController::new(claude_config, show_thinking);
        controller.init_work_dir(claude_session.work_dir.clone()).await;
        if let Some(ref sid) = resume_session_id {
            controller.set_pending_resume_session_id(Some(sid.clone())).await;
        }
        if let Some(ref ctx) = mcp_context {
            controller.set_mcp_context(ctx.clone()).await;
        }

        self.do_start_claude_session(channel_id, controller, claude_session, false, extra_args)
            .await
    }

    pub async fn resume_claude_session(
        &self,
        claude_session_id: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
        mcp_context: Option<McpContext>,
    ) -> anyhow::Result<(ClaudeSession, Arc<Mutex<ClaudeController>>)> {
        let mut claude_session = self
            .get_claude_session(claude_session_id)
            .ok_or_else(|| anyhow::anyhow!("ClaudeSession not found"))?;

        if claude_session.state == ClaudeSessionState::Dead {
            anyhow::bail!("This session is dead and cannot be resumed")
        }

        let channel_id = claude_session.channel_session_id.clone();

        // Sync work_dir from channel — user may have changed it via /cd after creation
        let channel = self
            .get_channel(&channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found"))?;
        claude_session.work_dir = channel.work_dir.clone();

        let controller = ClaudeController::new(claude_config, show_thinking);
        controller.init_work_dir(claude_session.work_dir.clone()).await;
        if let Some(ref ctx) = mcp_context {
            controller.set_mcp_context(ctx.clone()).await;
        }
        if let Some(ref csid) = claude_session.claude_session_id {
            controller.set_claude_session_id(Some(csid.clone())).await;
            controller.set_pending_resume_session_id(Some(csid.clone())).await;
        }

        self.do_start_claude_session(&channel_id, controller, claude_session, true, vec![])
            .await
    }

    /// Stop the active Claude session for the given channel.
    ///
    /// This function handles WebUI controllers and marks the DB state (active=false,
    /// state=Stopped, stopped_at=now). For platform channels (Feishu/Telegram), the
    /// actual controller lives in the platform's own data structures. Platform
    /// `StopSession` handlers (feishu/ws.rs, telegram/mod.rs) are responsible for
    /// dropping their own controller reference after calling this function to
    /// synchronize DB state — the controller's `Drop` implementation handles
    /// subprocess termination.
    pub async fn stop_active_claude_session(
        &self,
        channel_id: &str,
    ) -> anyhow::Result<()> {
        let active: Option<(String, ClaudeSession)> = self
            .claude_sessions
            .iter()
            .find(|entry| {
                let s = entry.value();
                s.channel_session_id == channel_id && s.active
            })
            .map(|entry| (entry.key().clone(), entry.value().clone()));

        if let Some((id, mut session)) = active {
            if let Some(mut runtime) = self.webui_runtimes.get_mut(channel_id) {
                if let Some(ref active_claude) = runtime.active_claude {
                    if active_claude.claude_session.id == id {
                        let ctrl = active_claude.controller.lock().await;
                        let _ = ctrl.stop_session().await;
                    }
                }
                // Abort any running poller for this channel
                if let Some(handle) = runtime.poll_abort_handle.take() {
                    handle.abort();
                }
                runtime.active_claude = None;
            }

            session.active = false;
            session.state = ClaudeSessionState::Stopped;
            session.stopped_at = Some(chrono::Utc::now());
            self.insert_claude_session(session);
            info!("Stopped ClaudeSession {} in channel {}", id, channel_id);
        }

        Ok(())
    }

    pub fn get_active_claude_session(&self, channel_id: &str) -> Option<ClaudeSession> {
        self.claude_sessions
            .iter()
            .find(|entry| {
                let s = entry.value();
                s.channel_session_id == channel_id && s.active
            })
            .map(|entry| entry.value().clone())
    }

    // ------------------------------------------------------------------
    // Encapsulated session state synchronization
    // ------------------------------------------------------------------

    /// Update the work directory for a channel and its active ClaudeSession.
    pub async fn switch_work_dir(
        &self,
        channel_id: &str,
        new_dir: PathBuf,
    ) -> anyhow::Result<()> {
        let new_dir_str = new_dir.to_string_lossy().to_string();

        if self.get_channel(channel_id).is_none() {
            anyhow::bail!("Channel not found");
        }

        self.update_channel_work_dir(channel_id, &new_dir_str);

        if let Some(active) = self.get_active_claude_session(channel_id) {
            let mut updated = active;
            updated.work_dir = new_dir_str;
            self.insert_claude_session(updated);
        }

        Ok(())
    }

    /// Stop the active Claude session for a channel and clear runtime state.
    pub async fn stop_channel_session(&self, channel_id: &str) -> anyhow::Result<()> {
        self.stop_active_claude_session(channel_id).await?;

        if let Some(mut runtime) = self.webui_runtimes.get_mut(channel_id) {
            runtime.active_claude = None;
        }

        Ok(())
    }


    pub async fn create_webui_channel(
        &self,
        title: &str,
        work_dir: &str,
    ) -> anyhow::Result<WebUIChannelRuntime> {
        let channel = ChannelSession::new_webui(title, work_dir);
        let runtime = WebUIChannelRuntime {
            channel_session: channel.clone(),
            active_claude: None,
            poll_abort_handle: None,
        };
        crate::db::insert_channel_session(&channel);
        self.channel_sessions
            .insert(channel.id.clone(), channel);
        self.webui_runtimes
            .insert(runtime.channel_session.id.clone(), runtime.clone());
        Ok(runtime)
    }

    /// Get or create the singleton WebUI channel, ensuring a runtime exists.
    /// Uses a creation lock to prevent duplicate channels on concurrent calls.
    pub async fn get_or_create_webui_channel(
        &self,
        title: &str,
        work_dir: &str,
    ) -> anyhow::Result<WebUIChannelRuntime> {
        let key = "webui:singleton".to_string();
        let lock = self
            .creation_locks
            .entry(key)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _guard = lock.lock().await;

        // Re-check after acquiring the lock in case another caller just created it
        for entry in self.channel_sessions.iter() {
            let c = entry.value();
            if c.platform == "webui" {
                let channel_id = c.id.clone();
                self.ensure_webui_runtime(&channel_id);
                return self
                    .get_webui_runtime(&channel_id)
                    .ok_or_else(|| anyhow::anyhow!("WebUI runtime not found after ensuring it"));
            }
        }

        // Still not found — create it
        self.create_webui_channel(title, work_dir).await
    }

    pub fn ensure_webui_runtime(&self, channel_id: &str) {
        if self.webui_runtimes.contains_key(channel_id) {
            return;
        }
        if let Some(channel) = self.get_channel(channel_id) {
            let runtime = WebUIChannelRuntime {
                channel_session: channel,
                active_claude: None,
                poll_abort_handle: None,
            };
            self.webui_runtimes.insert(channel_id.to_string(), runtime);
        }
    }

    pub fn get_webui_runtime(&self, channel_id: &str) -> Option<WebUIChannelRuntime> {
        self.webui_runtimes.get(channel_id).map(|r| r.clone())
    }

    pub fn set_webui_active_claude(
        &self,
        channel_id: &str,
        active: Option<ActiveClaudeRuntime>,
    ) {
        if let Some(mut runtime) = self.webui_runtimes.get_mut(channel_id) {
            // Abort any running poller when switching or clearing the active session
            if let Some(handle) = runtime.poll_abort_handle.take() {
                handle.abort();
            }
            runtime.active_claude = active;
        }
    }

    /// Set the poller AbortHandle for a WebUI channel.
    /// If a previous poller is still running, it is aborted before storing the new handle.
    pub fn set_webui_poll_handle(
        &self,
        channel_id: &str,
        handle: Option<tokio::task::AbortHandle>,
    ) {
        if let Some(mut runtime) = self.webui_runtimes.get_mut(channel_id) {
            if let Some(old) = runtime.poll_abort_handle.take() {
                old.abort();
            }
            runtime.poll_abort_handle = handle;
        }
    }

    pub fn load_from_db(&self) {
        let channels = crate::db::load_all_channel_sessions();
        for mut channel in channels {
            // Fix up old TUI channels that were stored with wrong source due to a
            // bug where new_platform's fallback arm mapped "tui" to SessionSource::WebUI.
            if channel.platform == "tui" && channel.source != crate::session::channel_model::SessionSource::TUI {
                channel.source = crate::session::channel_model::SessionSource::TUI;
                crate::db::insert_channel_session(&channel);
            }
            self.channel_sessions
                .insert(channel.id.clone(), channel);
        }

        let sessions = crate::db::load_all_claude_sessions();
        for mut session in sessions {
            if session.active {
                session.active = false;
                session.state = ClaudeSessionState::Stopped;
                session.stopped_at = Some(chrono::Utc::now());
                crate::db::update_claude_session_active(&session.id, false);
                crate::db::update_claude_session_state(&session.id, "stopped");
                crate::db::update_claude_session_stopped_at(
                    &session.id,
                    session.stopped_at,
                );
            }
            if session.updated_at.is_none() {
                session.updated_at = session.stopped_at.or(Some(session.created_at));
            }
            self.claude_sessions
                .insert(session.id.clone(), session);
        }

        info!(
            "Loaded {} channels and {} Claude sessions from DB",
            self.channel_sessions.len(),
            self.claude_sessions.len()
        );
    }
}

impl Default for ChannelSessionManager {
    fn default() -> Self {
        Self::new()
    }
}
