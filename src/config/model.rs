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
    /// Address to bind the HTTP server to. "127.0.0.1" for localhost only,
    /// "0.0.0.0" to allow LAN access.
    pub bind_address: String,
    /// CIDR allowlist for IP-based access control. When non-empty, only
    /// requests from matching IP ranges are accepted.
    /// Example: ["127.0.0.1", "192.168.1.0/24", "10.0.0.0/8"]
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    /// Token for WebUI access control. When set, the WebUI requires
    /// `?token=xxx` query param or `Authorization: Bearer xxx` header.
    /// `None` means token auth is disabled (backwards compatible).
    #[serde(default)]
    pub webui_token: Option<String>,
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
    Pi,
    CodeWhale,
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
    pub pi: AgentProviderConfig,
    pub codewhale: AgentProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProviderConfig {
    /// Whether this provider appears in /agents pickers and can be started.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl Default for AgentProviderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            default_args: None,
            mode: None,
            permission: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeishuConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    /// Require WebUI admin approval before allowing new chats to interact.
    pub require_pairing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
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
            bind_address: "127.0.0.1".to_string(),
            allowed_ips: Vec::new(),
            webui_token: None,
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
            AgentProvider::Pi => write!(f, "pi"),
            AgentProvider::CodeWhale => write!(f, "codew"),
        }
    }
}

impl AgentProvider {
    pub fn parse_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "cursor" => AgentProvider::Cursor,
            "pi" => AgentProvider::Pi,
            "codew" => AgentProvider::CodeWhale,
            _ => AgentProvider::Claude,
        }
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: AgentProvider::Claude,
            cli_path: "claude".to_string(),
            default_args: String::new(),
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
            pi: AgentProviderConfig::default(),
            codewhale: AgentProviderConfig::default(),
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
            AgentProvider::Pi => Self {
                provider: AgentProvider::Pi,
                cli_path: "pi".to_string(),
                default_args: String::new(),
                mode: "rpc".to_string(),
                permission: "prompt".to_string(),
            },
            AgentProvider::CodeWhale => Self {
                provider: AgentProvider::CodeWhale,
                cli_path: "codewhale".to_string(),
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
        // Clear Claude-specific flags that don't apply to other providers.
        if !matches!(self.provider, AgentProvider::Claude) {
            if self.default_args == "--dangerously-skip-permissions" {
                self.default_args.clear();
            }
        }
        self
    }
}

impl AgentProfiles {
    /// Whether a provider is enabled in the current configuration.
    pub fn is_provider_enabled(&self, provider: &AgentProvider) -> bool {
        match provider {
            AgentProvider::Claude => self.claude.enabled,
            AgentProvider::Cursor => self.cursor.enabled,
            AgentProvider::Pi => self.pi.enabled,
            AgentProvider::CodeWhale => self.codewhale.enabled,
        }
    }

    pub fn effective_config(&self) -> AgentConfig {
        self.config_for_provider(None)
    }

    pub fn config_for_provider(&self, provider: Option<AgentProvider>) -> AgentConfig {
        let selected = provider.unwrap_or_else(|| self.default.clone());
        let mut config = AgentConfig::default_for_provider(selected.clone());
        let profile = match selected {
            AgentProvider::Claude => &self.claude,
            AgentProvider::Cursor => &self.cursor,
            AgentProvider::Pi => &self.pi,
            AgentProvider::CodeWhale => &self.codewhale,
        };
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
            require_pairing: true,
        }
    }
}
