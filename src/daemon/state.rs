// Daemon state management
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct DaemonState {
    pub running: bool,
    pub session_active: bool,
    pub work_dir: String,
}

pub type SharedState = Arc<RwLock<DaemonState>>;
