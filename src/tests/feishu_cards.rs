use crate::platform::feishu::cards::{
    build_dir_picker_card, build_permission_card, build_select_card, build_session_history_card,
    build_text_card,
};
use crate::session::channel_model::{ClaudeSession, ClaudeSessionState};

#[test]
fn dir_picker_pagination_preserves_channel_and_receive_ids() {
    let dirs: Vec<(String, String)> = (0..41)
        .map(|idx| (format!("dir-{idx}"), format!("/tmp/dir-{idx}")))
        .collect();

    let card = build_dir_picker_card(&dirs, 0, "/tmp", "chat-123", "open_id", "open-456");

    let elements = card["body"]["elements"].as_array().unwrap();
    let next_button = elements
        .iter()
        .find(|element| {
            element["behaviors"][0]["value"]["cmd"].as_str() == Some("ll_page")
                && element["behaviors"][0]["value"]["page"].as_u64() == Some(1)
        })
        .expect("next page button should exist");
    let value = &next_button["behaviors"][0]["value"];

    assert_eq!(value["chat_id"].as_str(), Some("chat-123"));
    assert_eq!(value["receive_id_type"].as_str(), Some("open_id"));
    assert_eq!(value["receive_id"].as_str(), Some("open-456"));
}

#[test]
fn permission_select_and_text_cards_have_expected_schema() {
    let permission = build_permission_card("req-1", "Bash", "chat-1");
    assert_eq!(permission["schema"].as_str(), Some("2.0"));
    assert_eq!(
        permission["body"]["elements"][1]["behaviors"][0]["value"]["cmd"].as_str(),
        Some("allow")
    );

    let select = build_select_card("req-2", "Pick", &["A".to_string()], "chat-1");
    assert_eq!(
        select["body"]["elements"][1]["behaviors"][0]["value"]["option"].as_str(),
        Some("A")
    );

    let text = build_text_card("Title", "Body");
    assert_eq!(text["header"]["title"]["content"].as_str(), Some("Title"));
}

#[test]
fn session_history_resume_preserves_chat_and_receive_ids() {
    let now = chrono::Utc::now();
    let session = ClaudeSession {
        id: "session-1".to_string(),
        channel_session_id: "channel-internal-id".to_string(),
        provider: "claude".to_string(),
        title: "Past work".to_string(),
        work_dir: "/tmp/project".to_string(),
        active: false,
        state: ClaudeSessionState::Stopped,
        provider_session_id: Some("claude-session-1".to_string()),
        claude_session_id: Some("claude-session-1".to_string()),
        created_at: now,
        stopped_at: Some(now),
        updated_at: Some(now),
    };

    let card = build_session_history_card(&[session], "oc-chat", "open_id", "ou-user");
    let value = &card["body"]["elements"][2]["behaviors"][0]["value"];

    assert_eq!(value["cmd"].as_str(), Some("resume"));
    assert_eq!(value["session_id"].as_str(), Some("session-1"));
    assert_eq!(value["chat_id"].as_str(), Some("oc-chat"));
    assert_eq!(value["receive_id_type"].as_str(), Some("open_id"));
    assert_eq!(value["receive_id"].as_str(), Some("ou-user"));
}
