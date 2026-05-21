use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub log: LogConfig,
    pub claude: ClaudeConfig,
    pub feishu: FeishuConfig,
    pub telegram: TelegramConfig,
    /// Default working directory for gateway sessions.
    /// Used by /ll, /cd_default, and as the Feishu directory boundary.
    pub default_dir: String,
    /// Whether to display Claude's Thinking blocks in output.
    pub show_thinking: bool,
    /// Number of days to retain downloaded media files (images/files/audio).
    /// Files older than this will be cleaned up every 8 hours. Default: 30.
    pub media_retention_days: u64,
    /// Local port bound by the daemon to enforce a single instance.
    /// If the port is already in use, the daemon refuses to start.
    pub port: u16,
    /// Active platform integration (e.g. "feishu", "telegram").
    pub platform: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    pub file: String,
    pub max_lines: usize,
    pub max_size_mb: usize,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub allow_from: String,
    /// If set, use webhook mode instead of long-polling.
    pub webhook_url: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            log: LogConfig::default(),
            claude: ClaudeConfig::default(),
            feishu: FeishuConfig::default(),
            telegram: TelegramConfig::default(),
            default_dir: "~".to_string(),
            show_thinking: false,
            media_retention_days: 30,
            port: 17534,
            platform: "feishu".to_string(),
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            file: "~/.cc-gateway/logs/gateway.log".to_string(),
            max_lines: 100_000,
            max_size_mb: 50,
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

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: "${TELEGRAM_BOT_TOKEN}".to_string(),
            allow_from: "*".to_string(),
            webhook_url: "".to_string(),
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
}
