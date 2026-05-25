use crate::config::model::{ClaudeConfig, TelegramConfig};
use crate::db;
use crate::platform::telegram::TelegramPlatform;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

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
