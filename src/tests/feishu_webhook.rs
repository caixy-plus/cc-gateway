use super::helpers::{feishu_platform, feishu_text_event};

#[test]
fn feishu_webhook_challenge_echoes_plain_challenge() {
    let platform = feishu_platform("~");
    let response = platform
        .verify_challenge(&serde_json::json!({ "challenge": "plain-challenge" }))
        .unwrap();

    assert_eq!(response["challenge"], "plain-challenge");
}

#[tokio::test]
async fn feishu_webhook_deduplicates_repeated_im_events_before_processing() {
    let platform = feishu_platform("~");
    let event = feishu_text_event("msg-dup", "oc-chat", "p2p", "ou-user", "/pwd");

    platform.dedup_cache.insert("msg-dup".to_string());
    platform.handle_webhook_event(&event).await.unwrap();

    assert!(platform.dedup_cache.contains("msg-dup"));
    assert!(platform.channels.is_empty());
}

#[tokio::test]
async fn feishu_webhook_card_action_without_chat_id_is_safe_noop() {
    let platform = feishu_platform("~");
    let event = serde_json::json!({
        "header": { "event_type": "card.action.trigger" },
        "event": {
            "action": {
                "value": {
                    "cmd": "cd",
                    "path": "~"
                }
            }
        }
    });

    platform.handle_webhook_event(&event).await.unwrap();

    assert!(platform.channels.is_empty());
}
