use crate::config::model::{FeishuConfig, GatewayConfig};
use crate::platform::feishu::FeishuPlatform;
use serde_json::json;

fn test_platform() -> FeishuPlatform {
    let config = FeishuConfig {
        enabled: true,
        app_id: "${FEISHU_APP_ID}".to_string(),
        app_secret: "${FEISHU_APP_SECRET}".to_string(),
        require_pairing: false,
    };
    let gateway_config = GatewayConfig::default();
    let default_dir = &gateway_config.default_dir;
    FeishuPlatform::new(
        config,
        default_dir,
        gateway_config.agent.clone(),
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
