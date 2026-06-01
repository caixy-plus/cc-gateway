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
    let pending_perm: Arc<RwLock<Option<(String, String, String)>>> = Arc::new(RwLock::new(None));
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
    let pending_perm: Arc<RwLock<Option<(String, String, String)>>> = Arc::new(RwLock::new(None));
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

#[tokio::test]
async fn test_process_agent_event_sets_pending_permission() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pending_perm: Arc<RwLock<Option<(String, String, String)>>> = Arc::new(RwLock::new(None));
    let session_arc: Arc<RwLock<Option<AgentRuntime>>> = Arc::new(RwLock::new(None));
    let provider_session_id: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

    AgentController::process_agent_event(
        &tx,
        &pending_perm,
        &session_arc,
        &provider_session_id,
        AgentEvent::PermissionRequest {
            request_id: "req-1".to_string(),
            tool_name: "Bash".to_string(),
            input: None,
        },
        &Arc::new(AtomicBool::new(true)),
        "prompt",
    )
    .await;

    // Drain event
    let _ = rx.recv().await;

    // Wait for the tokio::spawn inside process_agent_event to execute
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    // pending_permission should be set
    let pending = pending_perm.read().await;
    assert!(pending.is_some());
    let (id, name, req_type) = pending.as_ref().unwrap();
    assert_eq!(id, "req-1");
    assert_eq!(name, "Bash");
    assert_eq!(req_type, "permission");
}

#[tokio::test]
async fn test_process_agent_event_sets_pending_for_confirm_select_question() {
    for (event, expected_type) in [
        (
            AgentEvent::ConfirmRequest {
                request_id: "confirm-1".to_string(),
                prompt: "Are you sure?".to_string(),
                options: vec!["yes".to_string(), "no".to_string()],
            },
            "confirm",
        ),
        (
            AgentEvent::SelectRequest {
                request_id: "select-1".to_string(),
                prompt: "Pick one".to_string(),
                options: vec!["a".to_string(), "b".to_string()],
            },
            "select",
        ),
        (
            AgentEvent::QuestionRequest {
                request_id: "question-1".to_string(),
                questions: vec![],
            },
            "question",
        ),
    ] {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let pending_perm: Arc<RwLock<Option<(String, String, String)>>> =
            Arc::new(RwLock::new(None));
        let session_arc: Arc<RwLock<Option<AgentRuntime>>> = Arc::new(RwLock::new(None));
        let provider_session_id: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

        AgentController::process_agent_event(
            &tx,
            &pending_perm,
            &session_arc,
            &provider_session_id,
            event,
            &Arc::new(AtomicBool::new(true)),
            "prompt",
        )
        .await;

        let _ = rx.recv().await;

        // Wait for the tokio::spawn inside process_agent_event to execute
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;

        let pending = pending_perm.read().await;
        assert!(pending.is_some(), "should set pending for {expected_type}");
        let (id, _name, req_type) = pending.as_ref().unwrap();
        assert_eq!(req_type, expected_type);
        assert!(!id.is_empty());
    }
}

#[tokio::test]
async fn test_get_pending_request_and_clear() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pending_perm: Arc<RwLock<Option<(String, String, String)>>> = Arc::new(RwLock::new(None));
    let session_arc: Arc<RwLock<Option<AgentRuntime>>> = Arc::new(RwLock::new(None));
    let provider_session_id: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));

    // Initially empty
    assert!(pending_perm.read().await.is_none());

    // Process a PermissionRequest event to set pending state
    AgentController::process_agent_event(
        &tx,
        &pending_perm,
        &session_arc,
        &provider_session_id,
        AgentEvent::PermissionRequest {
            request_id: "req-42".to_string(),
            tool_name: "Bash".to_string(),
            input: None,
        },
        &Arc::new(AtomicBool::new(true)),
        "prompt",
    )
    .await;

    let _ = rx.recv().await;

    // Wait for the tokio::spawn inside process_agent_event to execute
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;

    // Now pending should be set
    {
        let p = pending_perm.read().await;
        assert!(p.is_some());
        let (id, name, req_type) = p.as_ref().unwrap();
        assert_eq!(id, "req-42");
        assert_eq!(name, "Bash");
        assert_eq!(req_type, "permission");
    }

    // Clear it
    {
        let mut p = pending_perm.write().await;
        *p = None;
    }
    assert!(pending_perm.read().await.is_none());
}
