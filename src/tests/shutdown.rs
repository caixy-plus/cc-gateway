use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::command::router::CommandRouter;
use crate::config::model::ClaudeConfig;
use crate::platform::feishu::FeishuChannelRuntime;
use crate::platform::telegram::TelegramChannelRuntime;
use crate::session::channel_manager::ActiveClaudeRuntime;
use crate::session::channel_model::{
    ChannelSession, ClaudeSession, ClaudeSessionState, SessionSource,
};

fn channel(source: SessionSource, platform: &str, channel_id: &str) -> ChannelSession {
    ChannelSession {
        id: format!("channel-{channel_id}"),
        title: format!("{platform} chat"),
        source,
        platform: platform.to_string(),
        channel_id: channel_id.to_string(),
        work_dir: "~".to_string(),
        created_at: Utc::now(),
    }
}

fn dummy_active_runtime(channel_id: &str) -> ActiveClaudeRuntime {
    let controller = Arc::new(Mutex::new(ClaudeController::new(
        ClaudeConfig::default(),
        false,
    )));
    let router = Arc::new(CommandRouter::new(controller.clone(), "~"));
    ActiveClaudeRuntime {
        claude_session: ClaudeSession {
            id: "claude-row".to_string(),
            channel_session_id: channel_id.to_string(),
            title: "test".to_string(),
            work_dir: "~".to_string(),
            active: true,
            state: ClaudeSessionState::Active,
            claude_session_id: Some("cc-session".to_string()),
            created_at: Utc::now(),
            stopped_at: None,
            updated_at: Some(Utc::now()),
        },
        controller,
        router,
    }
}

#[test]
fn feishu_shutdown_notice_uses_receive_id_for_private_chats() {
    let mut runtime = FeishuChannelRuntime::new(
        channel(SessionSource::Feishu, "feishu", "oc_chat_id"),
        "open_id".to_string(),
        "ou_user_id".to_string(),
    );
    runtime.active_claude = Some(dummy_active_runtime("channel-oc_chat_id"));

    let target = runtime
        .shutdown_notice_target()
        .expect("active Feishu runtime should be notified");

    assert_eq!(target.receive_id_type, "open_id");
    assert_eq!(target.receive_id, "ou_user_id");
}

#[test]
fn telegram_shutdown_notice_targets_only_active_runtime() {
    let mut active =
        TelegramChannelRuntime::new(channel(SessionSource::Telegram, "telegram", "12345"));
    active.active_claude = Some(dummy_active_runtime("channel-12345"));
    let inactive =
        TelegramChannelRuntime::new(channel(SessionSource::Telegram, "telegram", "67890"));

    assert_eq!(active.shutdown_notice_chat_id(), Some(12345));
    assert_eq!(inactive.shutdown_notice_chat_id(), None);
}
