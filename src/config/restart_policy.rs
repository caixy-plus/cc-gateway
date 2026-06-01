use serde_json::{json, Value};

use crate::config::model::{AgentProfiles, FeishuConfig, GatewayConfig, LogConfig, TelegramConfig};

/// Describes which saved config changes need a daemon restart vs apply live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRestartAssessment {
    pub requires_restart: bool,
    /// Dot-path labels for UI, e.g. `port`, `feishu.enabled`.
    pub restart_fields: Vec<String>,
    pub live_fields: Vec<String>,
}

/// Metadata returned to the WebUI so it can show accurate hints without hard-coding.
pub fn restart_policy_metadata() -> Value {
    json!({
        "daemon_restart": daemon_restart_field_paths(),
        "live": live_field_paths(),
    })
}

pub fn daemon_restart_field_paths() -> Vec<&'static str> {
    vec![
        "port",
        "bind_address",
        "allowed_ips",
        "webui_token",
        "default_dir",
        "show_thinking",
        "media_retention_days",
        "session_retention_per_channel",
        "log",
        "agent",
        "feishu.enabled",
        "feishu.app_id",
        "feishu.app_secret",
        "telegram.enabled",
        "telegram.bot_token",
    ]
}

pub fn live_field_paths() -> Vec<&'static str> {
    vec!["feishu.require_pairing", "telegram.require_pairing"]
}

/// Compare config before/after a save merge.
pub fn assess_config_changes(
    before: &GatewayConfig,
    after: &GatewayConfig,
) -> ConfigRestartAssessment {
    let mut restart_fields = Vec::new();
    let mut live_fields = Vec::new();

    if before.port != after.port {
        restart_fields.push("port".to_string());
    }
    if before.bind_address != after.bind_address {
        restart_fields.push("bind_address".to_string());
    }
    if before.allowed_ips != after.allowed_ips {
        restart_fields.push("allowed_ips".to_string());
    }
    if before.webui_token != after.webui_token {
        restart_fields.push("webui_token".to_string());
    }
    if before.default_dir != after.default_dir {
        restart_fields.push("default_dir".to_string());
    }
    if before.show_thinking != after.show_thinking {
        restart_fields.push("show_thinking".to_string());
    }
    if before.media_retention_days != after.media_retention_days {
        restart_fields.push("media_retention_days".to_string());
    }
    if before.session_retention_per_channel != after.session_retention_per_channel {
        restart_fields.push("session_retention_per_channel".to_string());
    }
    if log_requires_restart(&before.log, &after.log) {
        restart_fields.push("log".to_string());
    }
    if agent_requires_restart(&before.agent, &after.agent) {
        restart_fields.push("agent".to_string());
    }
    feishu_restart_fields(
        &before.feishu,
        &after.feishu,
        &mut restart_fields,
        &mut live_fields,
    );
    telegram_restart_fields(
        &before.telegram,
        &after.telegram,
        &mut restart_fields,
        &mut live_fields,
    );

    let requires_restart = !restart_fields.is_empty();
    ConfigRestartAssessment {
        requires_restart,
        restart_fields,
        live_fields,
    }
}

fn log_requires_restart(before: &LogConfig, after: &LogConfig) -> bool {
    before.level != after.level
        || before.file != after.file
        || before.max_lines != after.max_lines
        || before.max_size_mb != after.max_size_mb
}

fn agent_requires_restart(before: &AgentProfiles, after: &AgentProfiles) -> bool {
    before != after
}

fn feishu_restart_fields(
    before: &FeishuConfig,
    after: &FeishuConfig,
    restart_fields: &mut Vec<String>,
    live_fields: &mut Vec<String>,
) {
    if before.enabled != after.enabled {
        restart_fields.push("feishu.enabled".to_string());
    }
    if before.app_id != after.app_id {
        restart_fields.push("feishu.app_id".to_string());
    }
    if before.app_secret != after.app_secret {
        restart_fields.push("feishu.app_secret".to_string());
    }
    if before.require_pairing != after.require_pairing {
        live_fields.push("feishu.require_pairing".to_string());
    }
}

fn telegram_restart_fields(
    before: &TelegramConfig,
    after: &TelegramConfig,
    restart_fields: &mut Vec<String>,
    live_fields: &mut Vec<String>,
) {
    if before.enabled != after.enabled {
        restart_fields.push("telegram.enabled".to_string());
    }
    if before.bot_token != after.bot_token {
        restart_fields.push("telegram.bot_token".to_string());
    }
    if before.require_pairing != after.require_pairing {
        live_fields.push("telegram.require_pairing".to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_only_change_does_not_require_restart() {
        let before = GatewayConfig::default();
        let mut after = before.clone();
        after.feishu.require_pairing = false;
        after.telegram.require_pairing = false;

        let assessment = assess_config_changes(&before, &after);
        assert!(!assessment.requires_restart);
        assert!(assessment
            .live_fields
            .contains(&"feishu.require_pairing".to_string()));
    }

    #[test]
    fn port_change_requires_restart() {
        let before = GatewayConfig::default();
        let mut after = before.clone();
        after.port = 9999;
        let assessment = assess_config_changes(&before, &after);
        assert!(assessment.requires_restart);
        assert!(assessment.restart_fields.contains(&"port".to_string()));
    }

    #[test]
    fn feishu_enable_requires_restart() {
        let before = GatewayConfig::default();
        let mut after = before.clone();
        after.feishu.enabled = false;
        let assessment = assess_config_changes(&before, &after);
        assert!(assessment.requires_restart);
        assert!(assessment
            .restart_fields
            .contains(&"feishu.enabled".to_string()));
    }
}
