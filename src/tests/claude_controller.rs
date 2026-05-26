use crate::claude::controller::{ensure_under_home, ClaudeController, ControllerEvent};
use crate::claude::protocol::OutputEvent;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

#[tokio::test]
async fn test_init_work_dir_sets_internal_state() {
    let config = crate::config::model::ClaudeConfig::default();
    let controller = ClaudeController::new(config, false);
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
    let config = crate::config::model::ClaudeConfig::default();
    let controller = ClaudeController::new(config, false);
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
async fn test_set_work_dir_outside_home_denied() {
    let config = crate::config::model::ClaudeConfig::default();
    let controller = ClaudeController::new(config, false);
    let result = controller.set_work_dir("/tmp".to_string()).await;
    assert!(
        result.is_err(),
        "Should deny changing work dir outside home"
    );
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
async fn test_set_work_dir_under_home_allowed() {
    let config = crate::config::model::ClaudeConfig::default();
    let controller = ClaudeController::new(config, false);
    let home = dirs::home_dir().unwrap();
    let test_dir = home.join("cc_gateway_test_nonexistent_12345");
    let result = controller
        .set_work_dir(test_dir.to_string_lossy().to_string())
        .await;
    // Validation should pass; start_session may fail due to missing Claude binary,
    // but it must NOT fail due to home directory restriction.
    if result.is_err() {
        let err = result.unwrap_err().to_string();
        assert!(
            !err.contains("outside home directory"),
            "Should not fail home check: {}",
            err
        );
    }
}

#[tokio::test]
async fn test_process_claude_event_emits_tool_result() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pending_perm: Arc<RwLock<Option<(String, String)>>> = Arc::new(RwLock::new(None));
    let session_arc: Arc<RwLock<Option<crate::claude::session::ClaudeSession>>> =
        Arc::new(RwLock::new(None));

    let event = OutputEvent::Assistant {
        message: crate::claude::protocol::AssistantMessage {
            role: "assistant".to_string(),
            content: vec![crate::claude::protocol::ContentBlock::ToolResult {
                content: Some("hello output".to_string()),
                is_error: false,
            }],
        },
    };
    ClaudeController::process_claude_event(
        &tx,
        &pending_perm,
        "/tmp",
        &session_arc,
        &Arc::new(RwLock::new(None)),
        event,
        &Arc::new(AtomicBool::new(true)),
    )
    .await;

    let received = rx.recv().await;
    assert!(
        matches!(received, Some(ControllerEvent::ToolResult(content, false)) if content == "hello output")
    );
}

#[tokio::test]
async fn test_process_claude_event_emits_tool_error() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let pending_perm: Arc<RwLock<Option<(String, String)>>> = Arc::new(RwLock::new(None));
    let session_arc: Arc<RwLock<Option<crate::claude::session::ClaudeSession>>> =
        Arc::new(RwLock::new(None));

    let event = OutputEvent::Assistant {
        message: crate::claude::protocol::AssistantMessage {
            role: "assistant".to_string(),
            content: vec![crate::claude::protocol::ContentBlock::ToolResult {
                content: Some("command failed".to_string()),
                is_error: true,
            }],
        },
    };
    ClaudeController::process_claude_event(
        &tx,
        &pending_perm,
        "/tmp",
        &session_arc,
        &Arc::new(RwLock::new(None)),
        event,
        &Arc::new(AtomicBool::new(true)),
    )
    .await;

    let received = rx.recv().await;
    assert!(
        matches!(received, Some(ControllerEvent::ToolResult(content, true)) if content == "command failed")
    );
}
