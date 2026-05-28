use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tracing::info;

use crate::command::router::CommandRouter;
use crate::config::model::{AgentProfiles, AgentProvider};
use crate::runtime::controller::AgentController;
use crate::runtime::event_poller::{AgentEventPoller, BufferedSink, EventPollSink};
use crate::session::channel_model::{
    AgentSession, AgentSessionState, ChannelSession, SessionSource,
};

// ---------------------------------------------------------------------------
// Active runtime — bundles the controller + router for an active Claude session
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ActiveAgentRuntime {
    pub agent_session: AgentSession,
    pub controller: Arc<Mutex<AgentController>>,
    pub router: Arc<CommandRouter>,
}

// ---------------------------------------------------------------------------
// WebUI channel runtime — extra fields for WebUI sessions
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct WebUIChannelRuntime {
    pub channel_session: ChannelSession,
    /// Active agent runtimes keyed by agent `session_id`.
    ///
    /// WebUI supports multiple sessions running concurrently under one channel.
    pub active_agents: Arc<DashMap<String, ActiveAgentRuntime>>,
    /// Per-session poller abort handles keyed by agent `session_id`.
    pub poll_handles: Arc<DashMap<String, Arc<Mutex<Option<AbortHandle>>>>>,
}

// ---------------------------------------------------------------------------
// Channel manager — global in-memory session store
// ---------------------------------------------------------------------------

pub struct ChannelManager {
    channels: DashMap<String, ChannelSession>,
    agent_sessions: DashMap<String, AgentSession>,
    webui_runtimes: DashMap<String, WebUIChannelRuntime>,
}

pub static GLOBAL_CHANNEL_SESSIONS: Lazy<ChannelManager> = Lazy::new(ChannelManager::new);

pub struct CreateAndStartAgentSessionArgs {
    pub channel_id: String,
    pub title: String,
    pub default_dir: String,
    pub agent_settings: AgentProfiles,
    pub show_thinking: bool,
    pub args: Vec<String>,
    pub resume_session_id: Option<String>,
    pub work_dir_override: Option<String>,
    pub mcp_context: Option<crate::runtime::mcp_server::McpContext>,
    pub provider_override: Option<crate::config::model::AgentProvider>,
}

pub struct StartAgentSessionForPlatformArgs {
    pub channel_id: String,
    pub title: String,
    pub default_dir: String,
    pub agent_settings: AgentProfiles,
    pub show_thinking: bool,
    pub args: Vec<String>,
    pub resume_session_id: Option<String>,
    pub work_dir_override: Option<String>,
    pub mcp_context: Option<crate::runtime::mcp_server::McpContext>,
    pub provider_override: Option<crate::config::model::AgentProvider>,
}

impl ChannelManager {
    fn new() -> Self {
        Self {
            channels: DashMap::new(),
            agent_sessions: DashMap::new(),
            webui_runtimes: DashMap::new(),
        }
    }

    // ------------------------------------------------------------------
    // Initialization
    // ------------------------------------------------------------------

    /// Load persisted sessions from SQLite on daemon startup.
    /// Mark previously-active sessions as inactive since their processes are gone.
    pub fn load_from_db(&self) {
        // Load channel sessions
        for channel in crate::db::load_all_channel_sessions() {
            self.channels.insert(channel.id.clone(), channel);
        }
        // Load claude sessions
        for mut session in crate::db::load_all_agent_sessions() {
            if session.active {
                session.active = false;
                session.state = AgentSessionState::Stopped;
                session.stopped_at = Some(Utc::now());
                crate::db::insert_agent_session(&session);
            }
            self.agent_sessions.insert(session.id.clone(), session);
        }
        info!(
            "Loaded {} channel sessions and {} claude sessions from DB",
            self.channels.len(),
            self.agent_sessions.len()
        );
    }

    #[cfg(test)]
    pub(crate) fn reset_for_tests(&self) {
        self.channels.clear();
        self.agent_sessions.clear();
        self.webui_runtimes.clear();
    }

    // ------------------------------------------------------------------
    // Channel management
    // ------------------------------------------------------------------

    pub fn get_channel(&self, id: &str) -> Option<ChannelSession> {
        self.channels.get(id).map(|e| e.clone())
    }

    pub fn list_channels(&self) -> Vec<ChannelSession> {
        self.channels.iter().map(|e| e.clone()).collect()
    }

    pub fn update_channel_work_dir(&self, id: &str, work_dir: &str) {
        if let Some(mut entry) = self.channels.get_mut(id) {
            entry.work_dir = work_dir.to_string();
        }
        crate::db::update_channel_work_dir(id, work_dir);
    }

    pub fn effective_channel_provider(
        &self,
        channel_id: &str,
        global: &AgentProfiles,
    ) -> AgentProvider {
        self.get_channel(channel_id)
            .and_then(|c| c.default_provider.clone())
            .map(|s| AgentProvider::parse_str(&s))
            .unwrap_or_else(|| global.default.clone())
    }

    pub fn set_channel_default_provider(
        &self,
        channel_id: &str,
        provider: AgentProvider,
    ) -> Result<()> {
        let provider_str = provider.to_string();
        let mut channel = self
            .get_channel(channel_id)
            .ok_or_else(|| anyhow::anyhow!("Channel not found: {}", channel_id))?;
        channel.default_provider = Some(provider_str.clone());
        self.channels.insert(channel_id.to_string(), channel);
        crate::db::update_channel_default_provider(channel_id, &provider_str);
        Ok(())
    }

    pub fn resolve_start_provider(
        &self,
        channel_id: &str,
        global: &AgentProfiles,
        explicit: Option<AgentProvider>,
    ) -> AgentProvider {
        explicit.unwrap_or_else(|| self.effective_channel_provider(channel_id, global))
    }

    pub async fn get_or_create_platform_channel(
        &self,
        platform: &str,
        channel_id: &str,
        default_dir: &str,
    ) -> ChannelSession {
        // Search by channel_id + platform
        for entry in self.channels.iter() {
            let c = entry.value();
            if c.platform == platform && c.channel_id == channel_id {
                return c.clone();
            }
        }
        let source = match platform {
            "feishu" => SessionSource::Feishu,
            "telegram" => SessionSource::Telegram,
            "tui" => SessionSource::Tui,
            _ => SessionSource::WebUI,
        };
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let channel = ChannelSession {
            id: id.clone(),
            title: format!("{} {}", platform, channel_id),
            source,
            platform: platform.to_string(),
            channel_id: channel_id.to_string(),
            work_dir: shellexpand::tilde(default_dir).to_string(),
            default_provider: None,
            created_at,
        };
        self.channels.insert(id.clone(), channel.clone());
        crate::db::insert_channel_session(&channel);
        channel
    }

    pub async fn get_or_create_webui_channel(
        &self,
        title: &str,
        default_dir: &str,
    ) -> Result<WebUIChannelRuntime> {
        // Search for existing WebUI channel
        for entry in self.channels.iter() {
            let c = entry.value();
            if c.platform == "webui" {
                if let Some(rt) = self.webui_runtimes.get(&c.id) {
                    return Ok(rt.clone());
                }
                let rt = WebUIChannelRuntime {
                    channel_session: c.clone(),
                    active_agents: Arc::new(DashMap::new()),
                    poll_handles: Arc::new(DashMap::new()),
                };
                self.webui_runtimes.insert(c.id.clone(), rt.clone());
                return Ok(rt);
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let channel = ChannelSession {
            id: id.clone(),
            title: title.to_string(),
            source: SessionSource::WebUI,
            platform: "webui".to_string(),
            channel_id: id.clone(),
            work_dir: shellexpand::tilde(default_dir).to_string(),
            default_provider: None,
            created_at,
        };
        self.channels.insert(id.clone(), channel.clone());
        crate::db::insert_channel_session(&channel);
        let rt = WebUIChannelRuntime {
            channel_session: channel,
            active_agents: Arc::new(DashMap::new()),
            poll_handles: Arc::new(DashMap::new()),
        };
        self.webui_runtimes.insert(id.clone(), rt.clone());
        Ok(rt)
    }

    // ------------------------------------------------------------------
    // Claude session management
    // ------------------------------------------------------------------

    pub fn list_agent_sessions(&self) -> Vec<AgentSession> {
        self.agent_sessions.iter().map(|e| e.clone()).collect()
    }

    pub fn list_agent_sessions_by_channel(
        &self,
        channel_id: &str,
        limit: Option<usize>,
    ) -> Vec<AgentSession> {
        let mut sessions: Vec<AgentSession> = self
            .agent_sessions
            .iter()
            .filter(|e| e.value().channel_session_id == channel_id)
            .map(|e| e.clone())
            .collect();
        // Sort by created_at descending (newest first)
        sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        if let Some(l) = limit {
            sessions.truncate(l);
        }
        sessions
    }

    pub fn get_agent_session(&self, id: &str) -> Option<AgentSession> {
        self.agent_sessions.get(id).map(|e| e.clone())
    }

    pub fn update_agent_session_work_dir(&self, id: &str, work_dir: &str) -> bool {
        let Some(mut session) = self.get_agent_session(id) else {
            return false;
        };
        session.work_dir = work_dir.to_string();
        session.updated_at = Some(Utc::now());
        self.agent_sessions
            .insert(session.id.clone(), session.clone());
        crate::db::insert_agent_session(&session);
        true
    }

    pub fn get_active_agent_session(&self, channel_id: &str) -> Option<AgentSession> {
        self.agent_sessions
            .iter()
            .find(|e| {
                let s = e.value();
                s.channel_session_id == channel_id && s.active
            })
            .map(|e| e.clone())
    }

    pub fn touch_agent_session(&self, id: &str) {
        if let Some(mut entry) = self.agent_sessions.get_mut(id) {
            entry.updated_at = Some(Utc::now());
            let session = entry.clone();
            drop(entry);
            crate::db::insert_agent_session(&session);
        }
    }

    /// Try to remove a Claude session. Returns `true` if deleted, `false` if
    /// the session is active and must be /quit first.
    pub fn remove_agent_session(&self, id: &str) -> bool {
        if let Some(s) = self.agent_sessions.get(id) {
            if s.active {
                return false;
            }
        }
        self.agent_sessions.remove(id);
        crate::db::delete_agent_session(id);
        true
    }

    pub fn create_agent_session_only(
        &self,
        channel_id: &str,
        title: &str,
        work_dir: &str,
        provider: &str,
    ) -> Result<AgentSession> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let session = AgentSession {
            id: id.clone(),
            channel_session_id: channel_id.to_string(),
            provider: provider.to_string(),
            title: title.to_string(),
            work_dir: work_dir.to_string(),
            active: false,
            state: AgentSessionState::Stopped,
            provider_session_id: None,
            created_at,
            stopped_at: None,
            updated_at: Some(created_at),
        };
        self.agent_sessions.insert(id.clone(), session.clone());
        crate::db::insert_agent_session(&session);
        Ok(session)
    }

    /// Record an already-active Claude session (e.g. TUI direct start) into memory and DB.
    pub fn record_active_agent_session(
        &self,
        channel_id: &str,
        title: &str,
        work_dir: &str,
        provider: &str,
        provider_session_id: Option<String>,
    ) -> Result<AgentSession> {
        self.stop_active_session_records_for_channel(channel_id, None);
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let session = AgentSession {
            id: id.clone(),
            channel_session_id: channel_id.to_string(),
            provider: provider.to_string(),
            title: title.to_string(),
            work_dir: work_dir.to_string(),
            active: true,
            state: AgentSessionState::Active,
            provider_session_id: provider_session_id.clone(),
            created_at,
            stopped_at: None,
            updated_at: Some(created_at),
        };
        self.agent_sessions.insert(id.clone(), session.clone());
        crate::db::insert_agent_session(&session);
        Ok(session)
    }

    pub async fn record_active_controller_session(
        &self,
        channel_id: &str,
        title: &str,
        controller: &Arc<Mutex<AgentController>>,
    ) -> Result<Option<AgentSession>> {
        let ctrl = controller.lock().await;
        if !ctrl.is_session_active().await {
            return Ok(None);
        }
        let work_dir = ctrl.get_work_dir().await;
        let provider_session_id = ctrl.get_provider_session_id().await;
        let provider = ctrl.provider_name().await;
        drop(ctrl);

        let session = self.record_active_agent_session(
            channel_id,
            title,
            &work_dir,
            &provider,
            provider_session_id,
        )?;
        Ok(Some(session))
    }

    pub async fn refresh_active_controller_session(
        &self,
        channel_id: &str,
        controller: &Arc<Mutex<AgentController>>,
    ) {
        let Some(mut session) = self.get_active_agent_session(channel_id) else {
            return;
        };
        let ctrl = controller.lock().await;
        session.work_dir = ctrl.get_work_dir().await;
        let provider = ctrl.provider_name().await;
        if let Some(provider_session_id) = ctrl.get_provider_session_id().await {
            session.provider_session_id = Some(provider_session_id);
        }
        session.provider = provider;
        session.updated_at = Some(Utc::now());
        drop(ctrl);

        self.agent_sessions
            .insert(session.id.clone(), session.clone());
        crate::db::insert_agent_session(&session);
    }

    pub async fn create_and_start_agent_session(
        &self,
        args: CreateAndStartAgentSessionArgs,
    ) -> Result<(AgentSession, Arc<Mutex<AgentController>>)> {
        // WebUI allows multiple sessions active concurrently under one channel.
        // Other platforms keep the invariant of at most one active session per channel.
        let is_webui_channel = self
            .get_channel(&args.channel_id)
            .map(|c| c.platform == "webui")
            .unwrap_or(false);
        if !is_webui_channel {
            self.stop_active_session_records_for_channel(&args.channel_id, None);
        }
        let work_dir = args
            .work_dir_override
            .or_else(|| self.get_channel(&args.channel_id).map(|c| c.work_dir))
            .unwrap_or_else(|| shellexpand::tilde(&args.default_dir).to_string());

        let controller = Arc::new(Mutex::new(AgentController::new(
            args.agent_settings.clone(),
            args.show_thinking,
        )));

        if let Some(ref sid) = args.resume_session_id {
            let ctrl = controller.lock().await;
            ctrl.set_pending_resume_session_id(Some(sid.clone())).await;
        }

        if let Some(ref mcp_ctx) = args.mcp_context {
            let ctrl = controller.lock().await;
            ctrl.set_mcp_context(mcp_ctx.clone()).await;
        }

        {
            let ctrl = controller.lock().await;
            ctrl.init_work_dir(work_dir.clone()).await;
            ctrl.start_session_with_provider(work_dir.clone(), args.args, args.provider_override)
                .await?;
        }

        let provider_session_id = {
            let ctrl = controller.lock().await;
            ctrl.get_provider_session_id().await
        };
        let provider = {
            let ctrl = controller.lock().await;
            ctrl.provider_name().await
        };

        self.update_channel_work_dir(&args.channel_id, &work_dir);

        let id = uuid::Uuid::new_v4().to_string();
        let created_at = Utc::now();
        let session = AgentSession {
            id: id.clone(),
            channel_session_id: args.channel_id,
            provider,
            title: args.title,
            work_dir,
            active: true,
            state: AgentSessionState::Active,
            provider_session_id: provider_session_id.clone(),
            created_at,
            stopped_at: None,
            updated_at: Some(created_at),
        };

        self.agent_sessions.insert(id.clone(), session.clone());
        crate::db::insert_agent_session(&session);

        Ok((session, controller))
    }

    /// High-level helper: create + start + build router in one call.
    /// Used by all platform integrations to reduce duplication.
    pub async fn start_agent_session_for_platform(
        &self,
        args: StartAgentSessionForPlatformArgs,
    ) -> Result<ActiveAgentRuntime> {
        let default_dir = args.default_dir.clone();
        let (agent_session, controller) = self
            .create_and_start_agent_session(CreateAndStartAgentSessionArgs {
                channel_id: args.channel_id,
                title: args.title,
                default_dir: args.default_dir,
                agent_settings: args.agent_settings,
                show_thinking: args.show_thinking,
                args: args.args,
                resume_session_id: args.resume_session_id,
                work_dir_override: args.work_dir_override,
                mcp_context: args.mcp_context,
                provider_override: args.provider_override,
            })
            .await?;
        let router = Arc::new(CommandRouter::new(controller.clone(), &default_dir));
        Ok(ActiveAgentRuntime {
            agent_session,
            controller,
            router,
        })
    }

    pub async fn resume_agent_session_runtime<C: Into<AgentProfiles>>(
        &self,
        session_id: &str,
        default_dir: &str,
        agent_settings: C,
        show_thinking: bool,
    ) -> Result<ActiveAgentRuntime> {
        self.resume_agent_session_for_platform(
            session_id,
            default_dir,
            agent_settings,
            show_thinking,
            None,
            None,
        )
        .await
    }

    pub async fn resume_agent_session_for_platform<C: Into<AgentProfiles>>(
        &self,
        session_id: &str,
        default_dir: &str,
        agent_settings: C,
        show_thinking: bool,
        work_dir_override: Option<String>,
        mcp_context: Option<crate::runtime::mcp_server::McpContext>,
    ) -> Result<ActiveAgentRuntime> {
        let agent_settings = agent_settings.into();
        let existing = self
            .get_agent_session(session_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;
        if let Some(ref override_dir) = work_dir_override {
            if override_dir != &existing.work_dir {
                tracing::info!(
                    "Resume session {} switching work_dir from current {} to original {}",
                    session_id,
                    override_dir,
                    existing.work_dir
                );
            }
        }
        let work_dir = existing.work_dir.clone();
        let stored_provider = existing.stored_provider();

        let controller = Arc::new(Mutex::new(AgentController::new(
            agent_settings.clone(),
            show_thinking,
        )));

        {
            let ctrl = controller.lock().await;
            // Cursor ACP session ids may be ephemeral; avoid attempting resume when the
            // session has no user messages recorded yet (common "never asked anything" case).
            let try_resume = match stored_provider {
                AgentProvider::Cursor => cursor_session_has_user_history(&existing),
                _ => true,
            };
            if try_resume {
                if let Some(sid) = existing.resume_provider_session_id() {
                    ctrl.set_pending_resume_session_id(Some(sid)).await;
                }
            }
            if let Some(ref mcp_ctx) = mcp_context {
                ctrl.set_mcp_context(mcp_ctx.clone()).await;
            }
            ctrl.init_work_dir(work_dir.clone()).await;
            ctrl.start_session_with_provider(
                work_dir.clone(),
                vec![],
                Some(stored_provider.clone()),
            )
            .await?;
        }

        let (new_provider_session_id, resumed_provider) = {
            let ctrl = controller.lock().await;
            (
                ctrl.get_provider_session_id().await,
                ctrl.provider_name().await,
            )
        };

        let mut session = existing;
        let is_webui_channel = self
            .get_channel(&session.channel_session_id)
            .map(|c| c.platform == "webui")
            .unwrap_or(false);
        if !is_webui_channel {
            self.stop_active_session_records_for_channel(
                &session.channel_session_id,
                Some(&session.id),
            );
        }
        session.active = true;
        session.state = AgentSessionState::Active;
        session.work_dir = work_dir.clone();
        session.provider = resumed_provider;
        session.provider_session_id = new_provider_session_id;
        session.updated_at = Some(Utc::now());
        session.stopped_at = None;

        self.update_channel_work_dir(&session.channel_session_id, &work_dir);
        self.agent_sessions
            .insert(session.id.clone(), session.clone());
        crate::db::insert_agent_session(&session);

        let router = Arc::new(CommandRouter::new(controller.clone(), default_dir));
        Ok(ActiveAgentRuntime {
            agent_session: session,
            controller,
            router,
        })
    }

    pub async fn resume_agent_session_with_controller(
        &self,
        session_id: &str,
        controller: &Arc<Mutex<AgentController>>,
    ) -> Result<AgentSession> {
        let existing = self
            .get_agent_session(session_id)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;
        let work_dir = existing.work_dir.clone();
        let stored_provider = existing.stored_provider();

        {
            let ctrl = controller.lock().await;
            if let Some(sid) = existing.resume_provider_session_id() {
                ctrl.set_pending_resume_session_id(Some(sid)).await;
            }
            let provider = ctrl
                .take_pending_resume_provider()
                .await
                .unwrap_or(stored_provider);
            ctrl.init_work_dir(work_dir.clone()).await;
            ctrl.start_session_with_provider(work_dir.clone(), vec![], Some(provider))
                .await?;
        }

        let (new_provider_session_id, resumed_provider) = {
            let ctrl = controller.lock().await;
            (
                ctrl.get_provider_session_id().await,
                ctrl.provider_name().await,
            )
        };

        let mut session = existing;
        let is_webui_channel = self
            .get_channel(&session.channel_session_id)
            .map(|c| c.platform == "webui")
            .unwrap_or(false);
        if !is_webui_channel {
            self.stop_active_session_records_for_channel(
                &session.channel_session_id,
                Some(&session.id),
            );
        }
        session.active = true;
        session.state = AgentSessionState::Active;
        session.work_dir = work_dir.clone();
        session.provider = resumed_provider;
        session.provider_session_id = new_provider_session_id;
        session.updated_at = Some(Utc::now());
        session.stopped_at = None;

        self.update_channel_work_dir(&session.channel_session_id, &work_dir);
        self.agent_sessions
            .insert(session.id.clone(), session.clone());
        crate::db::insert_agent_session(&session);

        Ok(session)
    }

    pub async fn send_and_poll_active_runtime_buffered<T: EventPollSink + Send>(
        &self,
        active: &ActiveAgentRuntime,
        text: &str,
        sink: &mut BufferedSink<T>,
    ) -> Result<()> {
        {
            let ctrl = active.controller.lock().await;
            ctrl.send_message(text).await?;
        }
        self.touch_agent_session(&active.agent_session.id);

        let poller = {
            let ctrl = active.controller.lock().await;
            AgentEventPoller::from_controller(&ctrl)
        };
        poller.run_buffered(sink).await
    }

    fn stop_active_session_records_for_channel(&self, channel_id: &str, except_id: Option<&str>) {
        let active_sessions: Vec<AgentSession> = self
            .agent_sessions
            .iter()
            .filter_map(|entry| {
                let session = entry.value();
                if session.channel_session_id == channel_id
                    && session.active
                    && except_id != Some(session.id.as_str())
                {
                    Some(session.clone())
                } else {
                    None
                }
            })
            .collect();

        for mut session in active_sessions {
            session.active = false;
            session.state = AgentSessionState::Stopped;
            session.stopped_at = Some(Utc::now());
            session.updated_at = Some(Utc::now());
            self.agent_sessions
                .insert(session.id.clone(), session.clone());
            crate::db::insert_agent_session(&session);
        }
    }

    pub async fn send_to_controller(
        &self,
        controller: &Arc<Mutex<AgentController>>,
        text: &str,
    ) -> Result<()> {
        let ctrl = controller.lock().await;
        ctrl.send_message(text).await
    }

    pub async fn stop_channel_session(&self, channel_id: &str) -> Result<()> {
        // WebUI: stop all active sessions for this channel.
        if let Some(channel) = self.get_channel(channel_id) {
            if channel.platform == "webui" {
                if let Some(rt) = self.webui_runtimes.get(channel_id) {
                    let active_ids: Vec<String> =
                        rt.active_agents.iter().map(|e| e.key().clone()).collect();
                    drop(rt);
                    for sid in active_ids {
                        let _ = self.stop_webui_session(channel_id, &sid).await;
                    }
                }
                return Ok(());
            }
        }

        // Other platforms: stop the single active session for this channel.
        if self.get_active_agent_session(channel_id).is_some() {
            let Some(mut session) = self.get_active_agent_session(channel_id) else {
                return Ok(());
            };
            session.active = false;
            session.state = AgentSessionState::Stopped;
            session.stopped_at = Some(Utc::now());
            session.updated_at = Some(Utc::now());
            self.agent_sessions
                .insert(session.id.clone(), session.clone());
            crate::db::insert_agent_session(&session);
        }
        Ok(())
    }

    pub async fn stop_active_runtime_for_channel(
        &self,
        channel_id: &str,
        active_runtime: Option<&ActiveAgentRuntime>,
    ) -> Result<()> {
        if let Some(active) = active_runtime {
            self.refresh_active_controller_session(channel_id, &active.controller)
                .await;
            let ctrl = active.controller.lock().await;
            let _ = ctrl.force_stop_session().await;
        }
        self.stop_channel_session(channel_id).await
    }

    pub async fn switch_work_dir(&self, channel_id: &str, dir: PathBuf) -> Result<()> {
        let dir_str = dir.to_string_lossy().to_string();
        self.update_channel_work_dir(channel_id, &dir_str);
        // Also update the active session's work_dir
        if let Some(active) = self.get_active_agent_session(channel_id) {
            let mut session = active;
            session.work_dir = dir_str;
            self.agent_sessions
                .insert(session.id.clone(), session.clone());
            crate::db::insert_agent_session(&session);
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // WebUI runtime management
    // ------------------------------------------------------------------

    pub fn get_webui_runtime(&self, channel_id: &str) -> Option<WebUIChannelRuntime> {
        self.webui_runtimes.get(channel_id).map(|e| e.clone())
    }

    pub fn get_webui_active_agent(
        &self,
        channel_id: &str,
        session_id: &str,
    ) -> Option<ActiveAgentRuntime> {
        let rt = self.webui_runtimes.get(channel_id)?;
        rt.active_agents.get(session_id).map(|e| e.clone())
    }

    pub fn set_webui_active_agent(&self, channel_id: &str, active: ActiveAgentRuntime) {
        if let Some(rt) = self.webui_runtimes.get_mut(channel_id) {
            rt.active_agents
                .insert(active.agent_session.id.clone(), active);
        }
    }

    pub fn remove_webui_active_agent(&self, channel_id: &str, session_id: &str) {
        if let Some(rt) = self.webui_runtimes.get_mut(channel_id) {
            rt.active_agents.remove(session_id);
        }
    }

    pub fn set_webui_poll_handle(&self, channel_id: &str, session_id: &str, handle: AbortHandle) {
        let poll_handles = match self.webui_runtimes.get(channel_id) {
            Some(rt) => rt.poll_handles.clone(),
            None => return,
        };
        let entry = poll_handles
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(None)))
            .clone();
        let lock_result = entry.try_lock();
        if let Ok(mut guard) = lock_result {
            if let Some(old) = guard.take() {
                old.abort();
            }
            *guard = Some(handle);
        }
    }

    pub async fn has_webui_poll_handle(&self, channel_id: &str, session_id: &str) -> bool {
        let rt = match self.webui_runtimes.get(channel_id) {
            Some(rt) => rt,
            None => return false,
        };
        let handle_arc = rt.poll_handles.get(session_id).map(|e| e.clone());
        drop(rt);
        match handle_arc {
            Some(handle) => handle.lock().await.is_some(),
            None => false,
        }
    }

    pub async fn stop_webui_session(&self, channel_id: &str, session_id: &str) -> Result<()> {
        let active = self.get_webui_active_agent(channel_id, session_id);
        if let Some(active) = active {
            self.refresh_active_controller_session(channel_id, &active.controller)
                .await;
            let ctrl = active.controller.lock().await;
            let _ = ctrl.force_stop_session().await;
        }

        // Abort poller for this session if present.
        if let Some(rt) = self.webui_runtimes.get(channel_id) {
            if let Some(handle_arc) = rt.poll_handles.get(session_id).map(|e| e.clone()) {
                if let Some(handle) = handle_arc.lock().await.take() {
                    handle.abort();
                }
            }
        }

        if let Some(mut session) = self.get_agent_session(session_id) {
            session.active = false;
            session.state = AgentSessionState::Stopped;
            session.stopped_at = Some(Utc::now());
            session.updated_at = Some(Utc::now());
            self.agent_sessions
                .insert(session.id.clone(), session.clone());
            crate::db::insert_agent_session(&session);
        }

        self.remove_webui_active_agent(channel_id, session_id);
        if let Some(rt) = self.webui_runtimes.get_mut(channel_id) {
            rt.poll_handles.remove(session_id);
        }
        Ok(())
    }
}

fn cursor_session_has_user_history(agent_session: &AgentSession) -> bool {
    let history_file_id = agent_session
        .provider_session_id
        .as_deref()
        .unwrap_or(&agent_session.id);
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return false,
    };
    let file_path = home
        .join(".cc-gateway")
        .join("history")
        .join(format!("{}.jsonl", history_file_id));
    file_contains_user_role(&file_path)
}

fn file_contains_user_role(path: &std::path::Path) -> bool {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // Cheap scan (one JSON per line). Avoid serde to keep this hot-path lightweight.
    content
        .lines()
        .any(|line| line.contains("\"role\":\"user\""))
}

#[cfg(test)]
mod cursor_resume_tests {
    use super::*;

    #[test]
    fn file_contains_user_role_detects_user_lines() {
        let dir = std::env::temp_dir().join(format!("cc-gateway-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.jsonl");
        std::fs::write(
            &path,
            r#"{"timestamp":"t","role":"assistant","content":"a"}
{"timestamp":"t","role":"user","content":"u"}
"#,
        )
        .unwrap();
        assert!(file_contains_user_role(&path));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_contains_user_role_false_when_missing_or_no_user() {
        let dir = std::env::temp_dir().join(format!("cc-gateway-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("y.jsonl");
        std::fs::write(&path, r#"{"role":"assistant","content":"a"}"#).unwrap();
        assert!(!file_contains_user_role(&path));
        assert!(!file_contains_user_role(&dir.join("missing.jsonl")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
