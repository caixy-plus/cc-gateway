use crate::agent::event::AgentEvent;
use crate::agent::session::AgentRuntime;
use crate::runtime::controller::{ensure_under_home, AgentController, ControllerEvent};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[tokio::test]
async fn test_init_work_dir_sets_internal_state() {
    let config = crate::config::model::AgentProfiles::default();
    let controller = AgentController::new(config, false);
    controller.init_work_dir("/test/path".to_string()).await;
    assert_eq!(controller.get_work_dir().await, "/test/path");
}

#[test]
fn test_ensure_under_home_allows_home_subdir() {
    let home = dirs::home_dir().unwrap();
    let test_path = home.join("some_subdir");
    let result = ensure_under_home(test_path.to_str().unwrap());
    assert!(
        result.is_ok(),
        "Should allow path under home: {:?}",
        result.err()
    );
    let resolved = result.unwrap();
    assert!(resolved.contains("some_subdir"));
}

#[test]
fn test_ensure_under_home_denies_outside_home() {
    let result = ensure_under_home("/tmp");
    assert!(result.is_err(), "Should deny path outside home");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Access denied"),
        "Error should mention access denied: {}",
        err
    );
    assert!(
        err.contains("outside home directory"),
        "Error should mention outside home: {}",
        err
    );
}

#[test]
fn test_ensure_under_home_denies_root() {
    let result = ensure_under_home("/");
    assert!(result.is_err(), "Should deny root directory");
}

#[tokio::test]
async fn test_start_session_outside_home_denied() {
    let config = crate::config::model::AgentProfiles::default();
    let controller = AgentController::new(config, false);
    let result = controller.start_session("/tmp".to_string(), vec![]).await;
    assert!(result.is_err(), "Should deny starting session outside home");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Access denied"),
        "Error should mention access denied: {}",
        err
    );
    assert!(
        err.contains("outside home directory"),
        "Error should mention outside home: {}",
        err
    );
}

#[tokio::test]
async fn test_init_work_dir_under_home_then_start_outside_home_denied() {
    let config = crate::config::model::AgentProfiles::default();
    let controller = AgentController::new(config, false);
    let home = dirs::home_dir().unwrap();
    let test_dir = home.join("cc_gateway_test_nonexistent_12345");
    controller
        .init_work_dir(test_dir.to_string_lossy().to_string())
        .await;
    let result = controller.start_session("/tmp".to_string(), vec![]).await;
    assert!(result.is_err(), "Should deny starting session outside home");
}

#[tokio::test]
async fn test_process_agent_event_emits_tool_result() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pending_perm: Arc<RwLock<Option<(String, String)>>> = Arc::new(RwLock::new(None));
    let session_arc: Arc<RwLock<Option<AgentRuntime>>> = Arc::new(RwLock::new(None));
    let provider_session_id: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

    AgentController::process_agent_event(
        &tx,
        &pending_perm,
        &session_arc,
        &provider_session_id,
        AgentEvent::ToolResult("hello output".to_string(), false),
        &Arc::new(AtomicBool::new(true)),
        "prompt",
    )
    .await;

    let received = rx.recv().await;
    assert!(
        matches!(received, Some(ControllerEvent::ToolResult(content, false)) if content == "hello output")
    );
}

#[tokio::test]
async fn test_process_agent_event_emits_tool_error() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pending_perm: Arc<RwLock<Option<(String, String)>>> = Arc::new(RwLock::new(None));
    let session_arc: Arc<RwLock<Option<AgentRuntime>>> = Arc::new(RwLock::new(None));
    let provider_session_id: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

    AgentController::process_agent_event(
        &tx,
        &pending_perm,
        &session_arc,
        &provider_session_id,
        AgentEvent::ToolResult("command failed".to_string(), true),
        &Arc::new(AtomicBool::new(true)),
        "prompt",
    )
    .await;

    let received = rx.recv().await;
    assert!(
        matches!(received, Some(ControllerEvent::ToolResult(content, true)) if content == "command failed")
    );
}
