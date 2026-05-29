use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub log: LogConfig,
    pub agent: AgentProfiles,
    pub feishu: FeishuConfig,
    pub telegram: TelegramConfig,
    /// Default working directory for gateway sessions.
    pub default_dir: String,
    /// Whether to display agent Thinking blocks in output.
    pub show_thinking: bool,
    pub media_retention_days: u64,
    /// Max agent sessions kept per channel by the background cleaner.
    pub session_retention_per_channel: u64,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    pub level: String,
    pub file: String,
    pub max_lines: usize,
    pub max_size_mb: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentProvider {
    #[default]
    Claude,
    Cursor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub provider: AgentProvider,
    pub cli_path: String,
    pub default_args: String,
    pub mode: String,
    pub permission: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentProfiles {
    pub default: AgentProvider,
    pub claude: AgentProviderConfig,
    pub cursor: AgentProviderConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeishuConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    pub encrypt_key: String,
    pub mode: String,
    pub webhook_bind: String,
    /// Require WebUI admin approval before allowing new chats to interact.
    pub require_pairing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    pub webhook_url: String,
    /// Require WebUI admin approval before allowing new chats to interact.
    pub require_pairing: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            log: LogConfig::default(),
            agent: AgentProfiles::default(),
            feishu: FeishuConfig::default(),
            telegram: TelegramConfig::default(),
            default_dir: "~".to_string(),
            show_thinking: false,
            media_retention_days: 30,
            session_retention_per_channel: 30,
            port: 17534,
        }
    }
}

pub const MIN_SESSION_RETENTION_PER_CHANNEL: u64 = 10;
pub const MAX_SESSION_RETENTION_PER_CHANNEL: u64 = 100;

pub fn effective_session_retention_per_channel(configured: u64) -> usize {
    configured.clamp(
        MIN_SESSION_RETENTION_PER_CHANNEL,
        MAX_SESSION_RETENTION_PER_CHANNEL,
    ) as usize
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

impl std::fmt::Display for AgentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentProvider::Claude => write!(f, "claude"),
            AgentProvider::Cursor => write!(f, "cursor"),
        }
    }
}

impl AgentProvider {
    pub fn parse_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "cursor" => AgentProvider::Cursor,
            _ => AgentProvider::Claude,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: AgentProvider::Claude,
            cli_path: "claude".to_string(),
            default_args: "--dangerously-skip-permissions".to_string(),
            mode: "agent".to_string(),
            permission: "prompt".to_string(),
        }
    }
}

impl Default for AgentProfiles {
    fn default() -> Self {
        Self {
            default: AgentProvider::Claude,
            claude: AgentProviderConfig::default(),
            cursor: AgentProviderConfig::default(),
        }
    }
}

impl AgentConfig {
    pub fn default_for_provider(provider: AgentProvider) -> Self {
        match provider {
            AgentProvider::Claude => Self::default(),
            AgentProvider::Cursor => Self {
                provider: AgentProvider::Cursor,
                cli_path: "agent".to_string(),
                default_args: String::new(),
                mode: "agent".to_string(),
                permission: "prompt".to_string(),
            },
        }
    }

    #[cfg(test)]
    pub fn with_provider_override(&self, provider: Option<AgentProvider>) -> Self {
        let Some(provider) = provider else {
            return self.clone().normalized();
        };
        if provider == self.provider {
            return self.clone().normalized();
        }
        let mut config = Self::default_for_provider(provider);
        config.mode = self.mode.clone();
        config.permission = self.permission.clone();
        config.normalized()
    }

    pub fn normalized(mut self) -> Self {
        if matches!(self.provider, AgentProvider::Cursor) {
            if self.cli_path.is_empty() || self.cli_path == "claude" {
                self.cli_path = "agent".to_string();
            }
            if self.default_args == "--dangerously-skip-permissions" {
                self.default_args.clear();
            }
        }
        if self.cli_path.is_empty() {
            self.cli_path = match self.provider {
                AgentProvider::Claude => "claude".to_string(),
                AgentProvider::Cursor => "agent".to_string(),
            };
        }
        self
    }
}

impl AgentProfiles {
    pub fn effective_config(&self) -> AgentConfig {
        self.config_for_provider(None)
    }

    pub fn config_for_provider(&self, provider: Option<AgentProvider>) -> AgentConfig {
        let selected = provider.unwrap_or_else(|| self.default.clone());
        let mut config = AgentConfig::default_for_provider(selected.clone());
        let profile = match selected {
            AgentProvider::Claude => &self.claude,
            AgentProvider::Cursor => &self.cursor,
        };
        if let Some(ref cli_path) = profile.cli_path {
            config.cli_path = cli_path.clone();
        }
        if let Some(ref default_args) = profile.default_args {
            config.default_args = default_args.clone();
        }
        if let Some(ref mode) = profile.mode {
            config.mode = mode.clone();
        }
        if let Some(ref permission) = profile.permission {
            config.permission = permission.clone();
        }
        config.normalized()
    }
}

impl GatewayConfig {
    pub fn effective_agent_settings(&self) -> AgentProfiles {
        self.agent.clone()
    }

    #[cfg(test)]
    pub fn effective_agent_config(&self) -> AgentConfig {
        self.agent.effective_config()
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: "${TELEGRAM_BOT_TOKEN}".to_string(),
            webhook_url: "".to_string(),
            require_pairing: true,
        }
    }
}

impl Default for FeishuConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            app_id: "${FEISHU_APP_ID}".to_string(),
            app_secret: "${FEISHU_APP_SECRET}".to_string(),
            encrypt_key: "".to_string(),
            mode: "websocket".to_string(),
            webhook_bind: "0.0.0.0:3000".to_string(),
            require_pairing: true,
        }
    }
}
