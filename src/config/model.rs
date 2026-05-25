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
