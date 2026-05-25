use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::router::{CommandAction, CommandRouter};
use crate::config::model::ClaudeConfig;

use super::helpers::TestEnv;

fn test_router() -> (CommandRouter, Arc<Mutex<ClaudeController>>) {
    test_router_with_default("~")
}

fn test_router_with_default(default_dir: &str) -> (CommandRouter, Arc<Mutex<ClaudeController>>) {
    let controller = Arc::new(Mutex::new(ClaudeController::new(
        ClaudeConfig::default(),
        false,
    )));
    (
        CommandRouter::new(controller.clone(), default_dir),
        controller,
    )
}

#[tokio::test]
async fn routes_claude_history_with_index_argument() {
    let (router, _) = test_router();
    let action = router.route("/claude-history 2").await;

    match action {
        CommandAction::ShowClaudeHistory { arg } => assert_eq!(arg, "2"),
        other => panic!("expected ShowClaudeHistory action, got {:?}", other),
    }
}

#[tokio::test]
async fn exposes_work_dir_after_relative_change_dir() {
    let env = TestEnv::new();
    let (router, controller) = test_router_with_default(env.home().to_str().unwrap());
    let temp = tempfile::tempdir_in(env.home()).expect("temp dir should be created");
    let child = temp.path().join("child");
    std::fs::create_dir(&child).expect("child dir should be created");
    let expected = temp.path().canonicalize().unwrap();

    {
        let ctrl = controller.lock().await;
        ctrl.init_work_dir(child.to_string_lossy().to_string())
            .await;
    }

    let reply = router
        .execute(CommandAction::ChangeDir(PathBuf::from("..")))
        .await;

    assert!(reply
        .unwrap()
        .contains(&expected.to_string_lossy().to_string()));
    assert_eq!(
        router.current_work_dir().await,
        expected.to_string_lossy().to_string()
    );
}

#[tokio::test]
async fn parses_core_commands_without_starting_claude() {
    let (router, _) = test_router();

    assert!(matches!(
        router.route("/ll src").await,
        CommandAction::ListDir { path: Some(_) }
    ));
    assert!(matches!(
        router.route("/claude --model sonnet").await,
        CommandAction::StartSession { args, .. } if args == vec!["--model", "sonnet"]
    ));
    assert!(matches!(
        router.route("/pwd").await,
        CommandAction::PrintWorkingDir
    ));
    assert!(matches!(
        router.route("/quit").await,
        CommandAction::Reply(_)
    ));
}
