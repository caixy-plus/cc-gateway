use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub log: LogConfig,
    pub claude: ClaudeConfig,
    pub feishu: FeishuConfig,
    /// Default working directory for gateway sessions.
    /// Used by /ll, /cd_default, and as the Feishu directory boundary.
    pub default_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub cli_path: String,
    pub default_args: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeishuConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    pub allow_from: String,
    pub encrypt_key: String,
    /// "websocket" or "webhook"
    pub mode: String,
    /// Bind address for webhook server (e.g. "0.0.0.0:3000")
    pub webhook_bind: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            log: LogConfig::default(),
            claude: ClaudeConfig::default(),
            feishu: FeishuConfig::default(),
            default_dir: "~".to_string(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: "~/.cc-gateway/logs/gateway.log".to_string(),
        }
    }
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            cli_path: "claude".to_string(),
            default_args: "--dangerously-skip-permissions".to_string(),
        }
    }
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            app_id: "${FEISHU_APP_ID}".to_string(),
            app_secret: "${FEISHU_APP_SECRET}".to_string(),
            allow_from: "*".to_string(),
            encrypt_key: "".to_string(),
            mode: "websocket".to_string(),
            webhook_bind: "0.0.0.0:3000".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn test_log_config_default() {
        let cfg = LogConfig::default();
        assert_eq!(cfg.level, "info");
        assert_eq!(cfg.file, "~/.cc-gateway/logs/gateway.log");
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
}
