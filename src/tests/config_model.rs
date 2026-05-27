use crate::config::model::{
    effective_session_retention_per_channel, ClaudeConfig, FeishuConfig, GatewayConfig, LogConfig,
    MAX_SESSION_RETENTION_PER_CHANNEL, MIN_SESSION_RETENTION_PER_CHANNEL,
};

#[test]
fn test_gateway_config_default() {
    let cfg = GatewayConfig::default();
    assert_eq!(cfg.log.level, "info");
    assert_eq!(cfg.log.file, "~/.cc-gateway/logs/gateway.log");
    assert_eq!(cfg.claude.cli_path, "claude");
    assert_eq!(cfg.claude.default_args, "--dangerously-skip-permissions");
    assert!(cfg.feishu.enabled);
    assert_eq!(cfg.feishu.app_id, "${FEISHU_APP_ID}");
    assert_eq!(cfg.feishu.app_secret, "${FEISHU_APP_SECRET}");
    assert_eq!(cfg.feishu.allow_from, "*");
    assert_eq!(cfg.feishu.encrypt_key, "");
    assert_eq!(cfg.default_dir, "~");
    assert_eq!(cfg.session_retention_per_channel, 30);
}

#[test]
fn effective_session_retention_per_channel_clamps_to_bounds_without_error() {
    assert_eq!(MIN_SESSION_RETENTION_PER_CHANNEL, 10);
    assert_eq!(MAX_SESSION_RETENTION_PER_CHANNEL, 100);
    assert_eq!(effective_session_retention_per_channel(30), 30);
    assert_eq!(effective_session_retention_per_channel(10), 10);
    assert_eq!(effective_session_retention_per_channel(100), 100);
    assert_eq!(effective_session_retention_per_channel(5), 10);
    assert_eq!(effective_session_retention_per_channel(0), 10);
    assert_eq!(effective_session_retention_per_channel(150), 100);
    assert_eq!(effective_session_retention_per_channel(u64::MAX), 100);
}

#[test]
fn test_log_config_default() {
    let cfg = LogConfig::default();
    assert_eq!(cfg.level, "info");
    assert_eq!(cfg.file, "~/.cc-gateway/logs/gateway.log");
    assert_eq!(cfg.max_lines, 100_000);
    assert_eq!(cfg.max_size_mb, 50);
}

#[test]
fn test_claude_config_default() {
    let cfg = ClaudeConfig::default();
    assert_eq!(cfg.cli_path, "claude");
    assert_eq!(cfg.default_args, "--dangerously-skip-permissions");
}

#[test]
fn test_feishu_config_default() {
    let cfg = FeishuConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.app_id, "${FEISHU_APP_ID}");
    assert_eq!(cfg.app_secret, "${FEISHU_APP_SECRET}");
    assert_eq!(cfg.allow_from, "*");
    assert_eq!(cfg.encrypt_key, "");
    assert_eq!(cfg.mode, "websocket");
    assert_eq!(cfg.webhook_bind, "0.0.0.0:3000");
}

#[test]
fn test_gateway_config_serde_roundtrip() {
    let original = GatewayConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: GatewayConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.log.level, deserialized.log.level);
    assert_eq!(original.claude.cli_path, deserialized.claude.cli_path);
    assert_eq!(original.feishu.allow_from, deserialized.feishu.allow_from);
    assert_eq!(original.default_dir, deserialized.default_dir);
    assert_eq!(original.show_thinking, deserialized.show_thinking);
}

#[test]
fn gateway_config_serialization_does_not_emit_legacy_platform_selector() {
    let value = serde_json::to_value(GatewayConfig::default()).unwrap();

    assert!(value.get("platform").is_none());
}

#[test]
fn gateway_config_ignores_legacy_platform_selector_when_loading() {
    let config: GatewayConfig = serde_json::from_value(serde_json::json!({
        "platform": "telegram",
        "feishu": { "enabled": true },
        "telegram": { "enabled": true },
    }))
    .unwrap();

    assert!(config.feishu.enabled);
    assert!(config.telegram.enabled);
}

#[test]
fn test_log_config_serde_roundtrip() {
    let original = LogConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: LogConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.level, deserialized.level);
    assert_eq!(original.file, deserialized.file);
    assert_eq!(original.max_lines, deserialized.max_lines);
    assert_eq!(original.max_size_mb, deserialized.max_size_mb);
}

#[test]
fn test_claude_config_serde_roundtrip() {
    let original = ClaudeConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: ClaudeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.cli_path, deserialized.cli_path);
    assert_eq!(original.default_args, deserialized.default_args);
}

#[test]
fn test_feishu_config_serde_roundtrip() {
    let original = FeishuConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: FeishuConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.enabled, deserialized.enabled);
    assert_eq!(original.app_id, deserialized.app_id);
    assert_eq!(original.app_secret, deserialized.app_secret);
    assert_eq!(original.allow_from, deserialized.allow_from);
    assert_eq!(original.encrypt_key, deserialized.encrypt_key);
    assert_eq!(original.mode, deserialized.mode);
    assert_eq!(original.webhook_bind, deserialized.webhook_bind);
}
