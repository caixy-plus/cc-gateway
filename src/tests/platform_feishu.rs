use crate::config::model::{FeishuConfig, GatewayConfig};
use crate::platform::feishu::FeishuPlatform;
use serde_json::json;

fn test_platform() -> FeishuPlatform {
    let config = FeishuConfig {
        enabled: true,
        app_id: "${FEISHU_APP_ID}".to_string(),
        app_secret: "${FEISHU_APP_SECRET}".to_string(),
        allow_from: "*".to_string(),
        encrypt_key: "".to_string(),
        mode: "websocket".to_string(),
        webhook_bind: "0.0.0.0:3000".to_string(),
    };
    let gateway_config = GatewayConfig::default();
    let default_dir = &gateway_config.default_dir;
    FeishuPlatform::new(
        config,
        default_dir,
        gateway_config.claude.clone(),
        gateway_config.show_thinking,
    )
}

#[tokio::test]
#[ignore = "requires network access to Feishu API"]
async fn test_get_tenant_access_token_with_real_credentials() {
    let platform = test_platform();
    let token = platform.get_tenant_access_token().await;
    assert!(token.is_ok(), "Failed to get token: {:?}", token.err());
    let token_str = token.unwrap();
    assert!(!token_str.is_empty(), "Token should not be empty");
    println!("Got Feishu tenant_access_token: {}", token_str);
}

#[tokio::test]
#[ignore = "requires network access to Feishu API"]
async fn test_refresh_token_with_real_credentials() {
    let platform = test_platform();
    let token = platform.token_manager.refresh_token().await;
    assert!(token.is_ok(), "Failed to refresh token: {:?}", token.err());
    let token_str = token.unwrap();
    assert!(!token_str.is_empty(), "Token should not be empty");
    println!("Refreshed Feishu tenant_access_token: {}", token_str);
}

#[tokio::test]
async fn test_token_caching_logic() {
    let platform = test_platform();
    {
        let cached = platform.token_manager.cached_token.read().await;
        assert!(cached.is_none());
    }
    let token = platform.get_tenant_access_token().await;
    if token.is_ok() {
        let cached = platform.token_manager.cached_token.read().await;
        assert!(cached.is_some());
        let fetched_at = platform.token_manager.token_fetched_at.read().await;
        assert!(fetched_at.is_some());
    }
}

#[tokio::test]
#[ignore = "requires network access to Feishu API"]
async fn test_list_chats_and_send_message() {
    let platform = test_platform();

    // List chats
    let chats = platform.list_chats().await;
    assert!(chats.is_ok(), "Failed to list chats: {:?}", chats.err());
    let chats = chats.unwrap();
    println!("Chats: {:?}", chats);

    // If there are chats, try sending a message to the first one
    if let Some(chat) = chats.first() {
        let result = platform
            .send_text_message("chat_id", &chat.chat_id, "Hello from cc-gateway test!")
            .await;
        assert!(result.is_ok(), "Failed to send message: {:?}", result.err());
        println!("Sent message to chat: {} ({})", chat.name, chat.chat_id);
    }
}

#[test]
fn test_verify_challenge() {
    let platform = test_platform();
    let body = json!({
        "challenge": "abc123",
        "token": "verification-token",
        "type": "url_verification"
    });
    let resp = platform.verify_challenge(&body).unwrap();
    assert_eq!(resp.get("challenge").unwrap().as_str().unwrap(), "abc123");
}

#[test]
fn test_verify_challenge_missing_field() {
    let platform = test_platform();
    let body = json!({
        "token": "verification-token",
        "type": "url_verification"
    });
    assert!(platform.verify_challenge(&body).is_err());
}

#[tokio::test]
async fn test_handle_webhook_event_text_message() {
    let platform = test_platform();
    let body = json!({
        "schema": "2.0",
        "header": {
            "event_id": "event-123",
            "event_type": "im.message.receive_v1",
            "create_time": "1234567890"
        },
        "event": {
            "message": {
                "message_id": "om_123",
                "message_type": "text",
                "content": "{\"text\":\"hello world\"}",
                "chat_id": "oc_123",
                "chat_type": "group"
            },
            "sender": {
                "sender_id": {
                    "open_id": "ou_123"
                },
                "sender_type": "user"
            }
        }
    });

    let result = platform.handle_webhook_event(&body).await;
    assert!(result.is_ok());
    let msg = result.unwrap();
    assert!(msg.is_some());
    let msg = msg.unwrap();
    assert_eq!(msg.message_id, "om_123");
    assert_eq!(msg.message_type, "text");
    assert_eq!(msg.sender_open_id, "ou_123");
    assert_eq!(msg.chat_id, Some("oc_123".to_string()));
}

#[tokio::test]
async fn test_handle_webhook_event_challenge_refused() {
    let platform = test_platform();
    let body = json!({
        "challenge": "abc123",
        "token": "verification-token",
        "type": "url_verification"
    });
    let result = platform.handle_webhook_event(&body).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_webhook_event_unhandled_type() {
    let platform = test_platform();
    let body = json!({
        "schema": "2.0",
        "header": {
            "event_id": "event-456",
            "event_type": "drive.file.created_v1"
        },
        "event": {}
    });
    let result = platform.handle_webhook_event(&body).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}
