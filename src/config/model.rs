use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    pub log: LogConfig,
    pub agent: AgentProfiles,
    pub feishu: FeishuConfig,
    pub telegram: TelegramConfig,
    pub qq: QqConfig,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    OpenCode,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentProfiles {
    pub default: AgentProvider,
    pub claude: AgentProviderConfig,
    pub cursor: AgentProviderConfig,
    pub pi: AgentProviderConfig,
    pub codewhale: AgentProviderConfig,
    pub opencode: AgentProviderConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FeishuConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    /// Require WebUI admin approval before allowing new chats to interact.
    pub require_pairing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    pub enabled: bool,
    pub bot_token: String,
    /// Require WebUI admin approval before allowing new chats to interact.
    pub require_pairing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QqConfig {
    pub enabled: bool,
    pub app_id: String,
    pub app_secret: String,
    /// Use QQ sandbox API hosts when true.
    pub sandbox: bool,
    pub require_pairing: bool,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            log: LogConfig::default(),
            agent: AgentProfiles::default(),
            feishu: FeishuConfig::default(),
            telegram: TelegramConfig::default(),
            qq: QqConfig::default(),
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
            AgentProvider::OpenCode => write!(f, "opencode"),
        }
    }
}

impl AgentProvider {
    pub fn parse_str(s: &str) -> Self {
        crate::config::agent_registry::parse_provider_id(s).unwrap_or(AgentProvider::Claude)
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
            opencode: AgentProviderConfig::default(),
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
            AgentProvider::OpenCode => Self {
                provider: AgentProvider::OpenCode,
                cli_path: "opencode".to_string(),
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
        if matches!(
            self.provider,
            AgentProvider::CodeWhale | AgentProvider::Pi | AgentProvider::OpenCode
        ) {
            self.default_args = strip_unsupported_default_args(&self.default_args);
        }
        self
    }
}

/// Flags meant for Cursor/Claude CLIs that break other providers (e.g. `codewhale serve --acp`).
fn strip_unsupported_default_args(args: &str) -> String {
    const UNSUPPORTED: &[&str] = &[
        "--yolo",
        "--print",
        "--force",
        "--permission-mode",
        "bypassPermissions",
        "--dangerously-skip-permissions",
    ];
    let kept: Vec<&str> = args
        .split_whitespace()
        .filter(|token| !UNSUPPORTED.iter().any(|bad| *bad == *token))
        .collect();
    kept.join(" ")
}

impl AgentProfiles {
    /// Whether a provider is enabled in the current configuration.
    pub fn is_provider_enabled(&self, provider: &AgentProvider) -> bool {
        match provider {
            AgentProvider::Claude => self.claude.enabled,
            AgentProvider::Cursor => self.cursor.enabled,
            AgentProvider::Pi => self.pi.enabled,
            AgentProvider::CodeWhale => self.codewhale.enabled,
            AgentProvider::OpenCode => self.opencode.enabled,
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
            AgentProvider::OpenCode => &self.opencode,
        };
        let explicit_permission = profile.permission.clone();
        if let Some(ref default_args) = profile.default_args {
            let (cli_args, semantics) = parse_gateway_default_args(default_args);
            config.default_args = cli_args;
            if explicit_permission.is_none() && semantics.yolo {
                config.permission = "allow".to_string();
            }
        }
        if let Some(ref mode) = profile.mode {
            config.mode = mode.clone();
        }
        if let Some(ref permission) = explicit_permission {
            config.permission = permission.clone();
        }
        config.normalized()
    }
}

/// Gateway-level aliases in `default_args` that must not be forwarded to provider CLIs.
struct GatewayDefaultArgsSemantics {
    yolo: bool,
}

fn parse_gateway_default_args(args: &str) -> (String, GatewayDefaultArgsSemantics) {
    let mut yolo = false;
    let kept: Vec<&str> = args
        .split_whitespace()
        .filter(|token| match *token {
            "--yolo" => {
                yolo = true;
                false
            }
            _ => true,
        })
        .collect();
    (kept.join(" "), GatewayDefaultArgsSemantics { yolo })
}

impl GatewayConfig {
    pub fn effective_agent_settings(&self) -> AgentProfiles {
        self.agent.clone()
    }

    #[cfg(test)]
    pub fn effective_agent_config(&self) -> AgentConfig {
        self.agent.effective_config()
    }

    /// In-memory defaults used by daemon / TUI / WebUI when `config.json` does not
    /// exist yet. Integrations stay disabled until `cc-gateway init` writes the file.
    pub fn runtime_defaults() -> Self {
        let mut config = Self::default();
        config.agent.claude.enabled = false;
        config.agent.cursor.enabled = false;
        config.agent.pi.enabled = false;
        config.agent.codewhale.enabled = false;
        config.agent.opencode.enabled = false;
        config.qq.enabled = false;
        config.qq.app_id.clear();
        config.qq.app_secret.clear();
        config.feishu.enabled = false;
        config.feishu.app_id.clear();
        config.feishu.app_secret.clear();
        config.telegram.enabled = false;
        config.telegram.bot_token.clear();
        config
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

impl Default for QqConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            app_id: "${QQ_APP_ID}".to_string(),
            app_secret: "${QQ_APP_SECRET}".to_string(),
            sandbox: false,
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
