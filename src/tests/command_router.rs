use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::command::router::{CommandAction, CommandRouter};
use crate::config::model::{AgentProfiles, AgentProvider};
use crate::runtime::controller::AgentController;

use super::helpers::TestEnv;

fn test_router() -> (CommandRouter, Arc<Mutex<AgentController>>) {
    test_router_with_default("~")
}

fn test_router_with_default(default_dir: &str) -> (CommandRouter, Arc<Mutex<AgentController>>) {
    let controller = Arc::new(Mutex::new(AgentController::new(
        AgentProfiles::default(),
        false,
    )));
    (
        CommandRouter::new(controller.clone(), default_dir),
        controller,
    )
}

fn display_path(path: &std::path::Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string()
}

#[tokio::test]
async fn routes_agent_history_with_index_argument() {
    let (router, _) = test_router();
    let action = router.route("/agent-history 2").await;

    match action {
        CommandAction::ShowAgentHistory { arg } => assert_eq!(arg, "2"),
        other => panic!("expected ShowAgentHistory action, got {:?}", other),
    }
}

#[tokio::test]
async fn routes_telegram_menu_aliases_with_underscores() {
    let (router, _) = test_router();

    assert!(matches!(
        router.route("/agent_history 2").await,
        CommandAction::ShowAgentHistory { arg } if arg == "2"
    ));
    assert!(matches!(
        router.route("/cd_up").await,
        CommandAction::ChangeDir(path) if path == PathBuf::from("..")
    ));
    assert!(matches!(
        router.route("/show_thinking").await,
        CommandAction::ShowThinking
    ));
    assert!(matches!(
        router.route("/hide_thinking").await,
        CommandAction::HideThinking
    ));
}

#[tokio::test]
async fn inactive_quit_reply_uses_i18n() {
    let _env = TestEnv::new();
    let previous_lang = std::env::var("CC_GATEWAY_LANG").ok();
    std::env::set_var("CC_GATEWAY_LANG", "zh_CN");
    crate::i18n::init();
    let (router, _) = test_router();

    let action = router.route("/quit").await;

    match action {
        CommandAction::Reply(message) => {
            assert_eq!(message, crate::t!("builtin.no_active_session_to_quit"));
        }
        other => panic!("expected translated reply, got {:?}", other),
    }

    if let Some(lang) = previous_lang {
        std::env::set_var("CC_GATEWAY_LANG", lang);
        crate::i18n::init();
    } else {
        std::env::set_var("CC_GATEWAY_LANG", "en");
        crate::i18n::init();
        std::env::remove_var("CC_GATEWAY_LANG");
    }
}

#[tokio::test]
async fn exposes_work_dir_after_relative_change_dir() {
    let env = TestEnv::new();
    let (router, controller) = test_router_with_default(env.home().to_str().unwrap());
    let temp = tempfile::tempdir_in(env.home()).expect("temp dir should be created");
    let child = temp.path().join("child");
    std::fs::create_dir(&child).expect("child dir should be created");
    let expected = display_path(&temp.path().canonicalize().unwrap());

    {
        let ctrl = controller.lock().await;
        ctrl.init_work_dir(child.to_string_lossy().to_string())
            .await;
    }

    let reply = router
        .execute(CommandAction::ChangeDir(PathBuf::from("..")))
        .await;

    assert!(reply.unwrap().contains(&expected));
    assert_eq!(router.current_work_dir().await, expected);
}

#[tokio::test]
async fn parses_core_commands_without_starting_claude() {
    let (router, _) = test_router();

    assert!(matches!(
        router.route("/ll src").await,
        CommandAction::ListDir { path: Some(_) }
    ));
    assert!(matches!(
        router.route("/agent --model sonnet").await,
        CommandAction::StartSession { provider: None, args, .. } if args == vec!["--model", "sonnet"]
    ));
    assert!(matches!(
        router.route("/cd").await,
        CommandAction::ListDir { path: None }
    ));
    assert!(matches!(
        router.route("/agent claude").await,
        CommandAction::StartSession { provider: Some(AgentProvider::Claude), args, .. } if args.is_empty()
    ));
    assert!(matches!(
        router.route("/agents cursor").await,
        CommandAction::SelectChannelAgent {
            provider: Some(AgentProvider::Cursor)
        }
    ));
    assert!(matches!(
        router.route("/agent codew").await,
        CommandAction::StartSession { provider: Some(AgentProvider::CodeWhale), args, .. } if args.is_empty()
    ));
    assert!(matches!(
        router.route("/agent cursor --force").await,
        CommandAction::StartSession { provider: Some(AgentProvider::Cursor), args, .. } if args == vec!["--force"]
    ));
    assert!(matches!(
        router.route("/agent claude --model sonnet").await,
        CommandAction::StartSession { provider: Some(AgentProvider::Claude), args, .. } if args == vec!["--model", "sonnet"]
    ));
    assert!(matches!(
        router.route("/agents").await,
        CommandAction::SelectChannelAgent { provider: None }
    ));
    assert!(matches!(
        router.route("/agents cursor").await,
        CommandAction::SelectChannelAgent {
            provider: Some(AgentProvider::Cursor)
        }
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

#[tokio::test]
async fn routes_allow_in_active_session() {
    let env = TestEnv::new();
    let (router, controller) = test_router_with_default(env.home().to_str().unwrap());
    {
        let ctrl = controller.lock().await;
        ctrl.start_session(env.home().to_string_lossy().to_string(), Vec::new())
            .await
            .expect("fake Claude session should start");
    }

    assert!(matches!(
        router.route("/allow").await,
        CommandAction::PermissionAllow { request_id: None }
    ));
    assert!(matches!(
        router.route("/allow abc123").await,
        CommandAction::PermissionAllow { request_id } if request_id.as_deref() == Some("abc123")
    ));

    {
        let ctrl = controller.lock().await;
        ctrl.stop_session().await.unwrap();
    }
}

#[tokio::test]
async fn routes_deny_in_active_session() {
    let env = TestEnv::new();
    let (router, controller) = test_router_with_default(env.home().to_str().unwrap());
    {
        let ctrl = controller.lock().await;
        ctrl.start_session(env.home().to_string_lossy().to_string(), Vec::new())
            .await
            .expect("fake Claude session should start");
    }

    assert!(matches!(
        router.route("/deny").await,
        CommandAction::PermissionDeny {
            request_id: None,
            reason: None
        }
    ));
    assert!(matches!(
        router.route("/deny abc123").await,
        CommandAction::PermissionDeny { request_id, reason: None } if request_id.as_deref() == Some("abc123")
    ));
    assert!(matches!(
        router.route("/deny abc123 not now").await,
        CommandAction::PermissionDeny { request_id, reason } if request_id.as_deref() == Some("abc123") && reason.as_deref() == Some("not now")
    ));

    {
        let ctrl = controller.lock().await;
        ctrl.stop_session().await.unwrap();
    }
}

#[tokio::test]
async fn allow_and_deny_reply_no_session_when_inactive() {
    let (router, _) = test_router();

    assert!(matches!(
        router.route("/allow").await,
        CommandAction::Reply(_)
    ));
    assert!(matches!(
        router.route("/deny").await,
        CommandAction::Reply(_)
    ));
}

#[tokio::test]
async fn active_agent_session_forwards_slash_commands_except_gateway_controls() {
    let env = TestEnv::new();
    let (router, controller) = test_router_with_default(env.home().to_str().unwrap());
    {
        let ctrl = controller.lock().await;
        ctrl.start_session(env.home().to_string_lossy().to_string(), Vec::new())
            .await
            .expect("fake Claude session should start");
    }

    assert!(matches!(
        router.route("/quit").await,
        CommandAction::StopSession
    ));
    assert!(matches!(
        router.route("/esc").await,
        CommandAction::FlushQueue { prompt: None }
    ));
    assert!(matches!(
        router.route("/esc /clear").await,
        CommandAction::FlushQueue { prompt } if prompt.as_deref() == Some("/clear")
    ));
    assert!(matches!(
        router.route("/stop").await,
        CommandAction::StopGeneration
    ));
    assert!(matches!(
        router.route("/clear").await,
        CommandAction::ClearSession
    ));
    assert!(matches!(
        router.route("/status").await,
        CommandAction::Status
    ));
    assert!(matches!(
        router.route("/show-thinking").await,
        CommandAction::ShowThinking
    ));
    assert!(matches!(
        router.route("/hide-thinking").await,
        CommandAction::HideThinking
    ));
    assert!(matches!(
        router.route("/pwd").await,
        CommandAction::ForwardToAgent(text) if text == "/pwd"
    ));
    assert!(matches!(
        router.route("/cd ..").await,
        CommandAction::ForwardToAgent(text) if text == "/cd .."
    ));
    assert!(matches!(
        router.route("/ll src").await,
        CommandAction::ForwardToAgent(text) if text == "/ll src"
    ));
    assert!(matches!(
        router.route("/agent-history").await,
        CommandAction::ForwardToAgent(text) if text == "/agent-history"
    ));
    assert!(matches!(
        router.route("/some-new-claude-command").await,
        CommandAction::ForwardToAgent(text) if text == "/some-new-claude-command"
    ));

    {
        let ctrl = controller.lock().await;
        ctrl.stop_session().await.unwrap();
    }
}
