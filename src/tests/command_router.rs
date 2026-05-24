use crate::claude::controller::ClaudeController;
use crate::command::router::CommandRouter;
use crate::config::model::ClaudeConfig;
use std::sync::Arc;
use tokio::sync::Mutex;

fn setup() -> CommandRouter {
    crate::i18n::init(); // ensure i18n dict is loaded for t!() macros
    let config = ClaudeConfig::default();
    let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
    CommandRouter::new(controller, "~")
}

#[tokio::test]
async fn test_help_handled_by_builtin() {
    let router = setup();
    let action = router.route("/help").await;
    let response = router.execute(action).await;
    assert!(response.is_some());
    let text = response.unwrap();
    assert!(text.contains("/help"));
}

#[tokio::test]
async fn test_pwd_handled_by_builtin() {
    let router = setup();
    let action = router.route("/pwd").await;
    let response = router.execute(action).await;
    assert!(response.is_some());
    let text = response.unwrap();
    // i18n key "builtin.current_dir" returns "Current directory: {DIR}" (En) or "当前目录: {DIR}" (ZhCN)
    assert!(text.contains("Current directory") || text.contains("当前目录"), "text was: {}", text);
}

#[tokio::test]
async fn test_slash_command_falls_through_when_inactive() {
    // Unknown slash commands when inactive return a help message
    let router = setup();
    let action = router.route("/clear").await;
    let response = router.execute(action).await;
    assert!(response.is_some());
    let text = response.unwrap();
    assert!(text.contains("Unknown command"));
}

#[tokio::test]
async fn test_regular_text_forwarded_when_inactive() {
    let router = setup();
    let action = router.route("hello world").await;
    eprintln!("DEBUG action={:?}", action);
    let response = router.execute(action).await;
    eprintln!("DEBUG response={:?}", response);
    assert!(response.is_some());
    let text = response.unwrap();
    // i18n key "forward.no_session" contains the phrase in En and ZhCN
    assert!(text.contains("No active Claude session") || text.contains("没有活动的 Claude 会话"), "text was: {}", text);
    assert!(text.contains("hello world"));
}

#[tokio::test]
async fn test_unknown_command_falls_through() {
    let router = setup();
    let action = router.route("/unknown_command_xyz").await;
    let response = router.execute(action).await;
    assert!(response.is_some());
    let text = response.unwrap();
    assert!(text.contains("Unknown command"));
    assert!(text.contains("/unknown_command_xyz"));
}

#[tokio::test]
async fn test_quit_handled_locally_when_session_active() {
    let config = ClaudeConfig::default();
    let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
    let router = CommandRouter::new(controller.clone(), "~");

    {
        let ctrl = controller.lock().await;
        ctrl.inject_dummy_session().await.unwrap();
    }

    let action = router.route("/quit").await;
    let response = router.execute(action).await;
    assert!(response.is_some(), "/quit should be handled locally when session is active");
}

#[tokio::test]
async fn test_ll_handled_locally_when_session_active() {
    let config = ClaudeConfig::default();
    let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
    let router = CommandRouter::new(controller.clone(), "~");

    {
        let ctrl = controller.lock().await;
        ctrl.inject_dummy_session().await.unwrap();
    }

    let action = router.route("/ll").await;
    assert!(
        matches!(action, crate::command::router::CommandAction::ListDir { .. }),
        "/ll should be handled locally when session is active, got: {:?}",
        action
    );

    {
        let ctrl = controller.lock().await;
        let _ = ctrl.stop_session().await;
    }
}

#[tokio::test]
async fn test_claude_replies_when_session_active() {
    let config = ClaudeConfig::default();
    let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
    let router = CommandRouter::new(controller.clone(), "~");

    {
        let ctrl = controller.lock().await;
        ctrl.inject_dummy_session().await.unwrap();
    }

    let action = router.route("/claude").await;
    let response = router.execute(action).await;
    assert!(
        response.is_some(),
        "/claude should return a reply when session is active, got: {:?}",
        response
    );
    let text = response.unwrap();
    assert!(
        text.contains("already active"),
        "Expected 'already active' message, got: {}",
        text
    );

    {
        let ctrl = controller.lock().await;
        let _ = ctrl.stop_session().await;
    }
}

#[tokio::test]
async fn test_text_forwarded_when_session_active() {
    let config = ClaudeConfig::default();
    let controller = Arc::new(Mutex::new(ClaudeController::new(config, false)));
    let router = CommandRouter::new(controller.clone(), "~");

    {
        let ctrl = controller.lock().await;
        ctrl.inject_dummy_session().await.unwrap();
    }

    let action = router.route("hello world").await;
    let response = router.execute(action).await;
    assert!(
        response.is_none(),
        "regular text should be forwarded to Claude when session is active, got: {:?}",
        response
    );

    {
        let ctrl = controller.lock().await;
        let _ = ctrl.stop_session().await;
    }
}
