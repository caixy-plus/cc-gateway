use crate::session::channel_model::{
    ChannelSession, ClaudeSession, ClaudeSessionState, SessionSource,
};

// ------------------------------------------------------------------
// ChannelSession construction
// ------------------------------------------------------------------

#[test]
fn test_channel_session_new_webui() {
    let cs = ChannelSession::new_webui("My Channel", "/home/user/workspace");
    assert_eq!(cs.title, "My Channel");
    assert_eq!(cs.work_dir, "/home/user/workspace");
    assert_eq!(cs.platform, "webui");
    assert_eq!(cs.source, SessionSource::WebUI);
    assert!(!cs.id.is_empty());
    assert!(!cs.channel_id.is_empty());
}

#[test]
fn test_channel_session_new_platform_feishu() {
    let cs = ChannelSession::new_platform("feishu", "chat_123", "/tmp");
    assert_eq!(cs.platform, "feishu");
    assert_eq!(cs.source, SessionSource::Feishu);
    assert_eq!(cs.channel_id, "chat_123");
    assert!(cs.title.contains("feishu"));
}

#[test]
fn test_channel_session_new_platform_telegram() {
    let cs = ChannelSession::new_platform("telegram", "tg_456", "/tmp");
    assert_eq!(cs.platform, "telegram");
    assert_eq!(cs.source, SessionSource::Telegram);
    assert_eq!(cs.channel_id, "tg_456");
}

#[test]
fn test_channel_session_new_platform_unknown_defaults_to_webui() {
    let cs = ChannelSession::new_platform("unknown", "x", "/tmp");
    assert_eq!(cs.platform, "unknown");
    assert_eq!(cs.source, SessionSource::WebUI);
}

#[test]
fn test_channel_session_new_tui() {
    let cs = ChannelSession::new_tui("/home/user");
    assert_eq!(cs.title, "TUI");
    assert_eq!(cs.platform, "tui");
    assert_eq!(cs.source, SessionSource::TUI);
    assert_eq!(cs.channel_id, "tui");
}

// ------------------------------------------------------------------
// ClaudeSession construction
// ------------------------------------------------------------------

#[test]
fn test_claude_session_new() {
    let s = ClaudeSession::new("channel-1", "Session A", "/home/user");
    assert_eq!(s.channel_session_id, "channel-1");
    assert_eq!(s.title, "Session A");
    assert_eq!(s.work_dir, "/home/user");
    assert!(!s.active);
    assert_eq!(s.state, ClaudeSessionState::Stopped);
    assert!(s.claude_session_id.is_none());
    assert!(s.stopped_at.is_none());
    assert!(!s.id.is_empty());
}

// ------------------------------------------------------------------
// ClaudeSessionState Display / FromStr
// ------------------------------------------------------------------

#[test]
fn test_claude_session_state_display() {
    assert_eq!(ClaudeSessionState::Active.to_string(), "active");
    assert_eq!(ClaudeSessionState::Stopped.to_string(), "stopped");
    assert_eq!(ClaudeSessionState::Dead.to_string(), "dead");
}

#[test]
fn test_claude_session_state_from_str() {
    assert_eq!(
        "active".parse::<ClaudeSessionState>().unwrap(),
        ClaudeSessionState::Active
    );
    assert_eq!(
        "stopped".parse::<ClaudeSessionState>().unwrap(),
        ClaudeSessionState::Stopped
    );
    assert_eq!(
        "dead".parse::<ClaudeSessionState>().unwrap(),
        ClaudeSessionState::Dead
    );
}

#[test]
fn test_claude_session_state_from_str_unknown() {
    assert!("unknown".parse::<ClaudeSessionState>().is_err());
}

#[test]
fn test_claude_session_state_partial_eq() {
    assert_eq!(ClaudeSessionState::Active, ClaudeSessionState::Active);
    assert_ne!(ClaudeSessionState::Active, ClaudeSessionState::Stopped);
}

// ------------------------------------------------------------------
// SessionSource equality
// ------------------------------------------------------------------

#[test]
fn test_session_source_equality() {
    assert_eq!(SessionSource::WebUI, SessionSource::WebUI);
    assert_ne!(SessionSource::WebUI, SessionSource::Feishu);
}

#[test]
fn test_session_source_serializes_to_string() {
    let json = serde_json::to_string(&SessionSource::WebUI).unwrap();
    assert_eq!(json, "\"WebUI\"", "SessionSource must serialize as string, got {}", json);
}
