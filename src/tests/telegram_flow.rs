use crate::config::model::{AgentProfiles, TelegramConfig};
use crate::db;
use crate::platform::telegram::TelegramPlatform;
use crate::runtime::file_delivery::McpDeliveryTarget;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::channel_model::{AgentSession, AgentSessionState};

use super::helpers::TestEnv;

fn telegram_platform(default_dir: &str) -> TelegramPlatform {
    TelegramPlatform::new(
        TelegramConfig {
            enabled: true,
            bot_token: "telegram-token".to_string(),
            require_pairing: false,
        },
        default_dir,
        AgentProfiles::default(),
        false,
    )
}

#[test]
fn telegram_api_url_uses_configured_bot_token() {
    let platform = telegram_platform("~");

    assert_eq!(
        platform.api_url("sendMessage"),
        "https://api.telegram.org/bottelegram-token/sendMessage"
    );
}

#[test]
fn telegram_mcp_context_targets_current_chat() {
    let platform = telegram_platform("~");
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

    let expected = [
        "help",
        "pwd",
        "ll",
        "cd",
        "cd_up",
        "cd_default",
        "mkdir",
        "agent",
        "agents",
        "agent_history",
        "show_thinking",
        "hide_thinking",
        "esc",
        "stop",
        "clear",
        "status",
        "quit",
    ];
    for name in expected {
        assert!(
            names.contains(&name),
            "missing telegram menu command: {}",
            name
        );
    }
    assert_eq!(names.len(), expected.len());
    assert!(names
        .iter()
        .all(|name| !name.contains('-') && !name.starts_with('/')));
    assert!(commands
        .iter()
        .all(|cmd| !cmd["description"].as_str().unwrap().is_empty()));
}

#[test]
fn telegram_directory_reply_markup_registers_callback_buttons() {
    let platform = telegram_platform("~");
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
    let platform = telegram_platform("~");
    let now = chrono::Utc::now();
    let sessions = vec![AgentSession {
        id: "session-1".to_string(),
        channel_session_id: "channel-1".to_string(),
        provider: "claude".to_string(),
        title: "Build feature".to_string(),
        work_dir: "/home/me/project".to_string(),
        active: false,
        state: AgentSessionState::Stopped,
        provider_session_id: Some("claude-1".to_string()),
        created_at: now,
        stopped_at: None,
        updated_at: Some(now),
    }];

    let markup = platform.history_reply_markup("12345", &sessions);

    let rows = markup["inline_keyboard"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].as_array().unwrap().len(), 3);
    assert_eq!(rows[0][0]["text"], crate::t!("telegram.resume"));
    assert_eq!(rows[0][1]["text"], crate::t!("telegram.start_new_session"));
    assert_eq!(rows[0][2]["text"], crate::t!("telegram.delete_session"));
    for button in rows[0].as_array().unwrap() {
        let callback_data = button["callback_data"].as_str().unwrap();
        assert!(callback_data.starts_with("cg:"));
        assert!(callback_data.len() <= 64);
    }
}

#[test]
fn telegram_history_message_includes_feishu_level_session_details() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-05-27T01:30:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let sessions = vec![AgentSession {
        id: "session-1".to_string(),
        channel_session_id: "channel-1".to_string(),
        provider: "claude".to_string(),
        title: "Build feature".to_string(),
        work_dir: "/home/me/project".to_string(),
        active: true,
        state: AgentSessionState::Active,
        provider_session_id: Some("claude-1".to_string()),
        created_at: now,
        stopped_at: None,
        updated_at: Some(now),
    }];

    let text = TelegramPlatform::history_message_text(&sessions);

    assert!(text.contains(crate::t!("telegram.session_history_subtitle")));
    assert!(text.contains("1. 🟢 Build feature"));
    assert!(text.contains("📁 /home/me/project"));
    assert!(text.contains("🕒 2026-05-27 09:30"));
    assert!(text.contains("🔑 claude-1"));
}

#[test]
fn telegram_history_message_shows_gateway_session_id_when_provider_id_missing() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-05-27T01:30:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let sessions = vec![AgentSession {
        id: "codewhale-session-1".to_string(),
        channel_session_id: "channel-1".to_string(),
        provider: "opencode".to_string(),
        title: "OpenCode work".to_string(),
        work_dir: "/home/me/project".to_string(),
        active: false,
        state: AgentSessionState::Stopped,
        provider_session_id: None,
        created_at: now,
        stopped_at: Some(now),
        updated_at: Some(now),
    }];

    let text = TelegramPlatform::history_message_text(&sessions);

    assert!(text.contains("🔑 codewhale-session-1"));
}

#[tokio::test]
async fn telegram_get_channel_reuses_runtime_and_persists_channel() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let platform = telegram_platform(env.home().to_str().unwrap());

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
