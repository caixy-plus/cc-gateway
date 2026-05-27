use crate::runtime::file_delivery::{
    telegram_send_document_url, validate_outbound_file, FeishuFileTarget, McpContext,
    McpDeliveryTarget, TelegramFileTarget,
};
use crate::runtime::mcp_server::send_file_tool_schema;

use super::helpers::TestEnv;

#[test]
fn mcp_delivery_target_round_trips_feishu_json_env() {
    let context = McpContext {
        delivery: McpDeliveryTarget::Feishu(FeishuFileTarget {
            app_id: "app-id".to_string(),
            app_secret: "app-secret".to_string(),
            chat_id: "ou-chat".to_string(),
            receive_id_type: "open_id".to_string(),
        }),
    };

    let encoded = context.to_env_json().unwrap();
    assert!(!encoded.contains("CC_GATEWAY_FEISHU"));

    let decoded = McpContext::from_env_json(&encoded).unwrap();
    assert_eq!(decoded, context);
}

#[test]
fn mcp_delivery_target_round_trips_telegram_json_env() {
    let context = McpContext {
        delivery: McpDeliveryTarget::Telegram(TelegramFileTarget {
            bot_token: "telegram-token".to_string(),
            chat_id: "12345".to_string(),
        }),
    };

    let encoded = context.to_env_json().unwrap();
    let decoded = McpContext::from_env_json(&encoded).unwrap();

    assert_eq!(decoded, context);
}

#[test]
fn telegram_send_document_url_uses_bot_token() {
    assert_eq!(
        telegram_send_document_url("telegram-token"),
        "https://api.telegram.org/bottelegram-token/sendDocument"
    );
}

#[test]
fn send_file_tool_schema_mentions_size_limit() {
    let schema = send_file_tool_schema();
    let description = schema["description"].as_str().unwrap();
    let path_description = schema["inputSchema"]["properties"]["path"]["description"]
        .as_str()
        .unwrap();

    assert!(description.contains("30MB"));
    assert!(path_description.contains("30MB"));
}

#[tokio::test]
async fn validate_outbound_file_rejects_invalid_paths_and_uses_default_filename() {
    let env = TestEnv::new();
    let dir = tempfile::tempdir_in(env.home()).unwrap();
    let file = dir.path().join("report.txt");
    std::fs::write(&file, "hello").unwrap();

    let outbound = validate_outbound_file(file.to_str().unwrap(), None)
        .await
        .unwrap();
    assert_eq!(outbound.file_name, "report.txt");
    assert_eq!(outbound.file_type, "stream");
    assert_eq!(outbound.bytes, b"hello");

    assert!(validate_outbound_file(dir.path().to_str().unwrap(), None)
        .await
        .is_err());
    assert!(
        validate_outbound_file(dir.path().join("missing.txt").to_str().unwrap(), None)
            .await
            .is_err()
    );
}
