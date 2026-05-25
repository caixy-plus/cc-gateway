use std::path::PathBuf;

use crate::command::router::CommandAction;
use crate::db;
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

use super::helpers::TestEnv;

#[tokio::test]
async fn chat_command_executor_starts_session_and_updates_channel_work_dir() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let root = env.home().join("chat-executor-root");
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("telegram", "chat-1", root.to_str().unwrap())
        .await;
    let executor =
        ChatCommandExecutor::new(root.to_str().unwrap(), env.fake_claude_config(), false);
    let mut context = ChatCommandContext::new(
        channel.id.clone(),
        "Telegram chat-1".to_string(),
        channel.work_dir.clone(),
        None,
    );

    let outcome = executor
        .execute(
            &mut context,
            CommandAction::StartSession {
                work_dir: Some(child.clone()),
                args: vec![],
            },
        )
        .await
        .unwrap();

    match outcome {
        ChatCommandOutcome::Started { work_dir, .. } => {
            assert_eq!(work_dir, child.to_string_lossy());
        }
        _ => panic!("expected started outcome"),
    }
    assert_eq!(context.channel_work_dir, child.to_string_lossy());
    assert!(context.active_claude.is_some());
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_channel(&channel.id)
            .unwrap()
            .work_dir,
        child.to_string_lossy()
    );

    GLOBAL_CHANNEL_SESSIONS
        .stop_active_runtime_for_channel(&channel.id, context.active_claude.as_ref())
        .await
        .unwrap();
}

#[tokio::test]
async fn chat_command_executor_changes_work_dir_without_platform_specific_code() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let root = env.home().join("chat-executor-cd");
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "oc-chat", root.to_str().unwrap())
        .await;
    let executor =
        ChatCommandExecutor::new(root.to_str().unwrap(), env.fake_claude_config(), false);
    let mut context = ChatCommandContext::new(
        channel.id.clone(),
        "Feishu oc-chat".to_string(),
        channel.work_dir.clone(),
        None,
    );

    let outcome = executor
        .execute(
            &mut context,
            CommandAction::ChangeDir(PathBuf::from("child")),
        )
        .await
        .unwrap();

    match outcome {
        ChatCommandOutcome::WorkDirChanged { work_dir, .. } => {
            assert_eq!(work_dir, child.to_string_lossy());
        }
        _ => panic!("expected workdir outcome"),
    }
    assert_eq!(context.channel_work_dir, child.to_string_lossy());
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_channel(&channel.id)
            .unwrap()
            .work_dir,
        child.to_string_lossy()
    );
}
