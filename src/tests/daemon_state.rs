use crate::daemon::state::{DaemonState, SharedState};
use std::sync::Arc;
use tokio::sync::RwLock;

#[test]
fn test_daemon_state_default() {
    let state = DaemonState::default();
    assert!(!state.running);
    assert!(!state.session_active);
    assert_eq!(state.work_dir, "");
}

#[tokio::test]
async fn test_shared_state_read_write() {
    let shared: SharedState = Arc::new(RwLock::new(DaemonState::default()));
    {
        let mut state = shared.write().await;
        state.running = true;
        state.session_active = true;
        state.work_dir = "/tmp".to_string();
    }
    {
        let state = shared.read().await;
        assert!(state.running);
        assert!(state.session_active);
        assert_eq!(state.work_dir, "/tmp");
    }
}

#[test]
fn test_daemon_state_clone() {
    let mut state = DaemonState::default();
    state.running = true;
    let cloned = state.clone();
    assert!(cloned.running);
}
