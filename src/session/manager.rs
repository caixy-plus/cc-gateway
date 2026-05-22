use std::sync::Arc;

use dashmap::DashMap;
use once_cell::sync::Lazy;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::router::CommandRouter;
use crate::config::model::ClaudeConfig;
use crate::session::model::Session;

#[derive(Clone)]
pub struct WebUISessionRuntime {
    pub session: Session,
    pub controller: Arc<Mutex<ClaudeController>>,
    pub router: Arc<CommandRouter>,
}

#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Session>>,
    webui_runtimes: Arc<DashMap<String, WebUISessionRuntime>>,
}

pub static GLOBAL_SESSIONS: Lazy<SessionManager> = Lazy::new(SessionManager::new);

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            webui_runtimes: Arc::new(DashMap::new()),
        }
    }

    pub fn insert(&self, session: Session) {
        crate::db::insert_session(&session);
        self.sessions.insert(session.id.clone(), session);
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).map(|s| s.clone())
    }

    pub fn remove(&self, id: &str) {
        crate::db::delete_session(id);
        self.sessions.remove(id);
        self.webui_runtimes.remove(id);
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions
            .iter()
            .map(|entry| entry.value().clone())
            .collect()
    }

    pub fn update_active(&self, id: &str, active: bool) {
        crate::db::update_active(id, active);
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.active = active;
        }
    }

    pub fn update_work_dir(&self, id: &str, work_dir: &str) {
        crate::db::update_work_dir(id, work_dir);
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.work_dir = work_dir.to_string();
        }
    }

    pub fn update_claude_session_id(&self, id: &str, claude_session_id: Option<&str>) {
        crate::db::update_claude_session_id(id, claude_session_id);
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.claude_session_id = claude_session_id.map(|s| s.to_string());
        }
    }

    pub async fn create_webui_session(
        &self,
        title: &str,
        work_dir: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
    ) -> anyhow::Result<WebUISessionRuntime> {
        let mut session = Session::new_webui(title, work_dir);
        session.active = false;
        let id = session.id.clone();
        let controller = Arc::new(Mutex::new(ClaudeController::new(
            claude_config,
            show_thinking,
        )));
        let router = Arc::new(CommandRouter::new(controller.clone(), work_dir));
        let runtime = WebUISessionRuntime {
            session: session.clone(),
            controller,
            router,
        };
        crate::db::insert_session(&session);
        self.sessions.insert(id.clone(), session);
        self.webui_runtimes.insert(id, runtime.clone());
        Ok(runtime)
    }

    pub fn load_sessions(&self) {
        let db_sessions = crate::db::load_all_sessions();
        for mut session in db_sessions {
            // After daemon restart, any previously active sessions are now inactive
            // because their Claude subprocesses are gone.
            if session.active {
                session.active = false;
                crate::db::update_active(&session.id, false);
            }
            self.sessions.insert(session.id.clone(), session);
        }
    }

    pub fn get_webui_runtime(&self, id: &str) -> Option<WebUISessionRuntime> {
        self.webui_runtimes.get(id).map(|r| r.clone())
    }

    pub async fn get_or_create_webui_runtime(
        &self,
        id: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
    ) -> Option<WebUISessionRuntime> {
        if let Some(runtime) = self.get_webui_runtime(id) {
            return Some(runtime);
        }
        let session = self.get(id)?;
        let controller = Arc::new(Mutex::new(ClaudeController::new(
            claude_config,
            show_thinking,
        )));
        {
            let ctrl = controller.lock().await;
            ctrl.init_work_dir(session.work_dir.clone()).await;
            if let Some(ref csid) = session.claude_session_id {
                ctrl.set_claude_session_id(Some(csid.clone())).await;
            }
        }
        let router = Arc::new(CommandRouter::new(controller.clone(), &session.work_dir));
        let runtime = WebUISessionRuntime {
            session: session.clone(),
            controller,
            router,
        };
        self.webui_runtimes.insert(id.to_string(), runtime.clone());
        Some(runtime)
    }

    pub fn remove_webui_runtime(&self, id: &str) {
        self.webui_runtimes.remove(id);
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
