use crate::config::loader::upgrade_config_json;
use crate::config::model::{
    effective_session_retention_per_channel, AgentConfig, AgentProfiles, AgentProvider,
    FeishuConfig, GatewayConfig, LogConfig, MAX_SESSION_RETENTION_PER_CHANNEL,
    MIN_SESSION_RETENTION_PER_CHANNEL,
};

#[test]
fn test_gateway_config_default() {
    let cfg = GatewayConfig::default();
    assert_eq!(cfg.log.level, "info");
    assert_eq!(cfg.log.file, "~/.cc-gateway/logs/gateway.log");
    assert_eq!(cfg.agent.default, AgentProvider::Claude);
    assert!(cfg.feishu.enabled);
    assert_eq!(cfg.feishu.app_id, "${FEISHU_APP_ID}");
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
fn test_agent_profiles_default() {
    let cfg = AgentProfiles::default();
    assert_eq!(cfg.default, AgentProvider::Claude);
    let agent = cfg.effective_config();
    assert_eq!(agent.cli_path, "claude");
    assert_eq!(agent.default_args, "");
}

#[test]
fn test_feishu_config_default() {
    let cfg = FeishuConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.app_id, "${FEISHU_APP_ID}");
}

#[test]
fn test_gateway_config_serde_roundtrip() {
    let original = GatewayConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: GatewayConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.log.level, deserialized.log.level);
    assert_eq!(original.agent.default, deserialized.agent.default);
    assert_eq!(original.feishu.require_pairing, deserialized.feishu.require_pairing);
    assert_eq!(original.default_dir, deserialized.default_dir);
}

#[test]
fn gateway_config_serialization_does_not_emit_legacy_platform_selector() {
    let value = serde_json::to_value(GatewayConfig::default()).unwrap();
    assert!(value.get("platform").is_none());
    assert!(value.get("claude").is_none());
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
fn gateway_config_upgrades_legacy_top_level_claude_block() {
    let config: GatewayConfig = serde_json::from_value(upgrade_config_json(serde_json::json!({
        "claude": {
            "cli_path": "custom-claude",
            "default_args": "--foo"
        }
    })))
    .unwrap();

    let agent = config.effective_agent_config();
    assert_eq!(agent.provider, AgentProvider::Claude);
    assert_eq!(agent.cli_path, "claude");
    assert_eq!(agent.default_args, "--foo");
}

#[test]
fn cursor_agent_config_normalizes_defaults() {
    let config: GatewayConfig = serde_json::from_value(upgrade_config_json(serde_json::json!({
        "agent": {
            "provider": "cursor"
        }
    })))
    .unwrap();

    let agent = config.effective_agent_config();
    assert_eq!(agent.provider, AgentProvider::Cursor);
    assert_eq!(agent.cli_path, "agent");
    assert_eq!(agent.default_args, "");
}

#[test]
fn nested_agent_config_selects_default_profile() {
    let config: GatewayConfig = serde_json::from_value(serde_json::json!({
        "agent": {
            "default": "cursor",
            "cursor": {
                "cli_path": "cursor-agent",
                "default_args": "--force",
                "mode": "plan",
                "permission": "allow"
            },
            "claude": {
                "cli_path": "custom-claude",
                "default_args": "--model sonnet"
            }
        }
    }))
    .unwrap();

    let agent = config.effective_agent_config();
    assert_eq!(agent.provider, AgentProvider::Cursor);
    assert_eq!(agent.cli_path, "agent");
    assert_eq!(agent.default_args, "--force");
    assert_eq!(agent.mode, "plan");
    assert_eq!(agent.permission, "allow");
}

#[test]
fn nested_agent_config_uses_named_profile_for_override() {
    let config: GatewayConfig = serde_json::from_value(serde_json::json!({
        "agent": {
            "default": "cursor",
            "cursor": {
                "cli_path": "cursor-agent",
                "default_args": "--force"
            },
            "claude": {
                "cli_path": "custom-claude",
                "default_args": "--model sonnet"
            }
        }
    }))
    .unwrap();

    let settings = config.effective_agent_settings();
    let claude = settings.config_for_provider(Some(AgentProvider::Claude));
    let cursor = settings.config_for_provider(Some(AgentProvider::Cursor));

    assert_eq!(claude.provider, AgentProvider::Claude);
    assert_eq!(claude.cli_path, "claude");
    assert_eq!(claude.default_args, "--model sonnet");
    assert_eq!(cursor.provider, AgentProvider::Cursor);
    assert_eq!(cursor.cli_path, "agent");
    assert_eq!(cursor.default_args, "--force");
}

#[test]
fn agent_config_serde_roundtrip() {
    let original = AgentConfig {
        provider: AgentProvider::Cursor,
        cli_path: "agent".to_string(),
        default_args: "--force".to_string(),
        mode: "plan".to_string(),
        permission: "allow".to_string(),
    };
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: AgentConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.provider, AgentProvider::Cursor);
    assert_eq!(deserialized.cli_path, "agent");
    assert_eq!(deserialized.default_args, "--force");
}

#[test]
fn provider_override_uses_target_provider_defaults() {
    let cursor_config = AgentConfig::default_for_provider(AgentProvider::Cursor);
    let claude_override = cursor_config.with_provider_override(Some(AgentProvider::Claude));

    assert_eq!(claude_override.provider, AgentProvider::Claude);
    assert_eq!(claude_override.cli_path, "claude");

    let cursor_override =
        AgentConfig::default().with_provider_override(Some(AgentProvider::Cursor));
    assert_eq!(cursor_override.provider, AgentProvider::Cursor);
    assert_eq!(cursor_override.cli_path, "agent");
    assert_eq!(cursor_override.default_args, "");
}

#[test]
fn test_log_config_serde_roundtrip() {
    let original = LogConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: LogConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.level, deserialized.level);
    assert_eq!(original.file, deserialized.file);
}

#[test]
fn test_agent_profiles_serde_roundtrip() {
    let original = AgentProfiles::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: AgentProfiles = serde_json::from_str(&json).unwrap();
    assert_eq!(original.default, deserialized.default);
}

#[test]
fn test_feishu_config_serde_roundtrip() {
    let original = FeishuConfig::default();
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: FeishuConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(original.enabled, deserialized.enabled);
    assert_eq!(original.app_id, deserialized.app_id);
}
