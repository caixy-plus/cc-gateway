use crate::claude::file_delivery::McpDeliveryTarget;
use crate::config::model::{ClaudeConfig, TelegramConfig};
use crate::db;
use crate::platform::telegram::TelegramPlatform;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::channel_model::{ClaudeSession, ClaudeSessionState};

use super::helpers::TestEnv;

fn telegram_platform(allow_from: &str, default_dir: &str) -> TelegramPlatform {
    TelegramPlatform::new(
        TelegramConfig {
            enabled: true,
            bot_token: "telegram-token".to_string(),
            allow_from: allow_from.to_string(),
            webhook_url: String::new(),
        },
        default_dir,
        ClaudeConfig::default(),
        false,
    )
}

#[test]
fn telegram_authorization_matches_user_id_username_and_wildcard() {
    let explicit = telegram_platform("12345, alice", "~");
    assert!(explicit.is_allowed_sender(12345, "bob"));
    assert!(explicit.is_allowed_sender(67890, "alice"));
    assert!(!explicit.is_allowed_sender(67890, "mallory"));

    let wildcard = telegram_platform("*", "~");
    assert!(wildcard.is_allowed_sender(1, "anyone"));
}

#[test]
fn telegram_api_url_uses_configured_bot_token() {
    let platform = telegram_platform("*", "~");

    assert_eq!(
        platform.api_url("sendMessage"),
        "https://api.telegram.org/bottelegram-token/sendMessage"
    );
}

#[test]
fn telegram_mcp_context_targets_current_chat() {
    let platform = telegram_platform("*", "~");
    let context = platform.mcp_context_for_chat("12345");

    match context.delivery {
        McpDeliveryTarget::Telegram(target) => {
            assert_eq!(target.bot_token, "telegram-token");
            assert_eq!(target.chat_id, "12345");
        }
        other => panic!("expected Telegram target, got {:?}", other),
    }
}

#[test]
fn telegram_bot_commands_payload_uses_valid_menu_commands() {
    let payload = TelegramPlatform::bot_commands_payload();
    let commands = payload["commands"].as_array().unwrap();
    let names: Vec<&str> = commands
        .iter()
        .map(|cmd| cmd["command"].as_str().unwrap())
        .collect();

    assert!(names.contains(&"help"));
    assert!(names.contains(&"ll"));
    assert!(names.contains(&"cd_up"));
    assert!(!names.contains(&"cd"));
    assert!(names.contains(&"claude"));
    assert!(names.contains(&"claude_history"));
    assert!(names.contains(&"show_thinking"));
    assert!(names.contains(&"hide_thinking"));
    assert!(names
        .iter()
        .all(|name| !name.contains('-') && !name.starts_with('/')));
    assert!(commands
        .iter()
        .all(|cmd| !cmd["description"].as_str().unwrap().is_empty()));
}

#[test]
fn telegram_directory_reply_markup_registers_callback_buttons() {
    let platform = telegram_platform("*", "~");
    let markup = platform.directory_reply_markup(
        "12345",
        &[
            ("project".to_string(), "/home/me/project".to_string()),
            ("downloads".to_string(), "/home/me/downloads".to_string()),
        ],
    );

    let rows = markup["inline_keyboard"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0]["text"], "project/");
    let callback_data = rows[0][0]["callback_data"].as_str().unwrap();
    assert!(callback_data.starts_with("cg:"));
    assert!(callback_data.len() <= 64);
}

#[test]
fn telegram_history_reply_markup_registers_callback_buttons() {
    let platform = telegram_platform("*", "~");
    let now = chrono::Utc::now();
    let sessions = vec![ClaudeSession {
        id: "session-1".to_string(),
        channel_session_id: "channel-1".to_string(),
        title: "Build feature".to_string(),
        work_dir: "/home/me/project".to_string(),
        active: false,
        state: ClaudeSessionState::Stopped,
        claude_session_id: Some("claude-1".to_string()),
        created_at: now,
        stopped_at: None,
        updated_at: Some(now),
    }];

    let markup = platform.history_reply_markup("12345", &sessions);

    let rows = markup["inline_keyboard"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0][0]["text"]
        .as_str()
        .unwrap()
        .contains("Build feature"));
    let callback_data = rows[0][0]["callback_data"].as_str().unwrap();
    assert!(callback_data.starts_with("cg:"));
    assert!(callback_data.len() <= 64);
}

#[tokio::test]
async fn telegram_get_channel_reuses_runtime_and_persists_channel() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let platform = telegram_platform("*", env.home().to_str().unwrap());

    let first = platform.get_channel("12345").await;
    let second = platform.get_channel("12345").await;

    assert_eq!(first.channel_session.id, second.channel_session.id);
    assert_eq!(first.channel_session.platform, "telegram");
    assert_eq!(first.channel_session.channel_id, "12345");
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_channel(&first.channel_session.id)
            .unwrap()
            .channel_id,
        "12345"
    );
}
