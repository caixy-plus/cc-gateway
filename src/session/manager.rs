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
        self.sessions.insert(session.id.clone(), session);
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions.get(id).map(|s| s.clone())
    }

    pub fn remove(&self, id: &str) {
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
        if let Some(mut session) = self.sessions.get_mut(id) {
            session.active = active;
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
        self.sessions.insert(id.clone(), session);
        self.webui_runtimes.insert(id, runtime.clone());
        Ok(runtime)
    }

    pub fn get_webui_runtime(&self, id: &str) -> Option<WebUISessionRuntime> {
        self.webui_runtimes.get(id).map(|r| r.clone())
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
