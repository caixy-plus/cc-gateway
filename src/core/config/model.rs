//! Gateway configuration data models.
//!
//! This module defines all structs deserialized from `~/.cc-gateway/config.json`,
//! including:
//!
//! - [`GatewayConfig`]: Top-level configuration (log, agent, platforms, port, permission, WebUI, etc.).
//! - [`AgentProvider`] and [`AgentConfig`]: Provider enum + runtime configuration for a single provider
//!   (cli path, default_args, mode, permission).
//! - [`AgentProfiles`]: `agent.default` + `agent.providers` mapping, corresponding to the agent list
//!   in WebUI "Settings".
//! - [`FeishuConfig`] / [`TelegramConfig`] / [`QqConfig`]: Configurations for the three chat platforms.
//!
//! By design, "registration info" ([`super::agent_registry`], hardcoded) is separated from "runtime configuration"
//! (this module, from the user's `config.json`). They are associated via provider id.

use std::collections::BTreeMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Configuration set of each chat platform (`config.json` → `"platforms": { "feishu": ... }`).
///
/// Each platform has an independent `enabled` flag, which can be enabled in any combination.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformsMap {
    pub feishu: FeishuConfig,
    pub telegram: TelegramConfig,
    pub qq: QqConfig,
}

/// Top-level configuration of the gateway, corresponding to the complete structure deserialized from `~/.cc-gateway/config.json`.
///
/// Uses `#[serde(default)]` to ensure no errors on old or missing fields, automatically filling in default values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Log settings (level, file path, rotation thresholds).
    pub log: LogConfig,
    /// Agent profiles: default provider + runtime configuration for each provider.
    pub agent: AgentProfiles,
    /// Three chat platforms (Feishu / Telegram / QQ).
    pub platforms: PlatformsMap,
    /// Default working directory (used when creating a session and no work_dir is explicitly passed).
    pub default_dir: String,
    /// Whether to display the agent's Thinking block in the output.
    pub show_thinking: bool,
    /// Number of days to retain chat attachments (images/files), cleaned up by background cleaner when expired.
    pub media_retention_days: u64,
    /// Maximum number of agent sessions to retain per channel (used by cleaner).
    pub session_retention_per_channel: u64,
    /// HTTP / WebUI listening port.
    pub port: u16,
    /// Listening address: `127.0.0.1` for local only, `0.0.0.0` to expose to LAN.
    pub bind_address: String,
    /// Access IP whitelist (CIDR format); empty means no IP restriction.
    #[serde(default)]
    pub allowed_ips: Vec<String>,
    /// WebUI access token; if set, WebUI must carry it in `?token=xxx` or `Authorization: Bearer xxx`.
    /// `None` means token authentication is disabled (backward compatible with old configs).
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

/// All registered agent providers in the gateway.
///
/// Each variant corresponds to a provider adapter layer **compiled into the gateway** (`src/core/agent/<name>.rs`).
/// To add a new provider, you must:
///
/// 1. Add a variant to this enum;
/// 2. Add a registration entry in `agent_registry::AGENT_PROVIDER_DEFS`;
/// 3. Add a corresponding branch in [`crate::core::agent::session::AgentRuntime`] in `src/core/agent/session.rs`;
/// 4. Add a corresponding branch in the `dispatch_agent_backend!` macro.
///
/// Serialized format is lowercase id (`claude` / `codex` / `cursor` / `pi` / `opencode` / `kimi`
/// / `gemini` / `qoder`), which is exactly consistent with the keys of `agent.providers.<id>` in `config.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentProvider {
    #[default]
    Claude,
    Codex,
    Cursor,
    Pi,
    OpenCode,
    Kimi,
    Gemini,
    Qoder,
}

/// The actual runtime configuration of a single provider (the final form of `config.json` → `agent.providers.<id>` merged with registry default values).
///
/// This is the struct that the `daemon` actually uses to spawn provider child processes. It is complementary to the registry's
/// [`super::agent_registry::AgentProviderDef`]: the registry describes "what the provider is",
/// while this struct describes "how the user wants to run it".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    /// Current provider.
    pub provider: AgentProvider,
    /// CLI binary path / name (can be overridden by the user in the profile).
    pub cli_path: String,
    /// Additional CLI arguments at startup (already gateway-level normalized: stripped of flags that are not common across providers).
    pub default_args: String,
    /// Provider mode (e.g., Claude's `agent` / `plan`, Codex's `auto` / `full-auto`).
    pub mode: String,
    /// Permission policy: `prompt` / `allow` / `deny`.
    pub permission: String,
}

/// The complete form of the `agent` field in `config.json`: `default` + `providers` mapping.
///
/// `providers` uses the provider id (lowercase) as the key, each key mapping to an [`AgentProviderConfig`].
/// Old flat formats (`agent.claude`, `agent.codex` ...) will be automatically migrated to `agent.providers.<id>`
/// by `upgrade_config_json` during configuration loading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProfiles {
    /// Default provider id. When a user inputs only `/agent` without specifying a provider in the chat,
    /// this provider is used to start the session.
    pub default: AgentProvider,
    /// Runtime configuration map indexed by provider id.
    #[serde(default)]
    pub providers: BTreeMap<String, AgentProviderConfig>,
}

/// Single provider profile (`config.json` → `agent.providers.<id>`).
///
/// All fields are `Option` because **default values are provided by the registry**: only fields explicitly
/// overridden by the user appear on disk, avoiding hardcoding defaults into every user configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentProviderConfig {
    /// Whether this provider appears in the `/agents` list and init wizard;
    /// if disabled, the gateway will not allow creating sessions for this provider.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// `default_args` input by the user in WebUI Settings (e.g., `--yolo`, `-m auto`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_args: Option<String>,
    /// Mode explicitly overridden by the user (omitted uses the registry default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Permission explicitly overridden by the user (omitted uses the registry default;
    /// automatically mapped to `allow` if `--yolo` is present in `default_args`).
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
    /// Optional HTTP/SOCKS proxy for Telegram Bot API only (e.g. `http://127.0.0.1:7890`).
    pub proxy: String,
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
            platforms: PlatformsMap::default(),
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
            AgentProvider::Codex => write!(f, "codex"),
            AgentProvider::Cursor => write!(f, "cursor"),
            AgentProvider::Pi => write!(f, "pi"),
            AgentProvider::OpenCode => write!(f, "opencode"),
            AgentProvider::Kimi => write!(f, "kimi"),
            AgentProvider::Gemini => write!(f, "gemini"),
            AgentProvider::Qoder => write!(f, "qoder"),
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

impl AgentConfig {
    pub fn default_for_provider(provider: AgentProvider) -> Self {
        match provider {
            AgentProvider::Claude => Self::default(),
            // `codex-acp` ignores `mode` in session/new and defaults to read-only;
            // "auto" is applied post-spawn via session/set_mode (Codex's own default preset).
            AgentProvider::Codex => Self {
                provider: AgentProvider::Codex,
                cli_path: "codex-acp".to_string(),
                default_args: String::new(),
                mode: "auto".to_string(),
                permission: "prompt".to_string(),
            },
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
            AgentProvider::OpenCode => Self {
                provider: AgentProvider::OpenCode,
                cli_path: "opencode".to_string(),
                default_args: String::new(),
                mode: "agent".to_string(),
                permission: "prompt".to_string(),
            },
            AgentProvider::Kimi => Self {
                provider: AgentProvider::Kimi,
                cli_path: "kimi".to_string(),
                default_args: String::new(),
                mode: "agent".to_string(),
                permission: "prompt".to_string(),
            },
            AgentProvider::Gemini => Self {
                provider: AgentProvider::Gemini,
                cli_path: "gemini".to_string(),
                default_args: String::new(),
                mode: "agent".to_string(),
                permission: "prompt".to_string(),
            },
            AgentProvider::Qoder => Self {
                provider: AgentProvider::Qoder,
                cli_path: "qoderclicn".to_string(),
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
        self.default_args = crate::config::agent_registry::normalize_default_args_for_provider(
            &self.provider,
            &self.default_args,
        );
        self
    }
}

/// Pi-only flags that break gateway session resume (`switch_session`); stripped silently.
const PI_STRIPPED_CLI_ARGS: &[&str] = &["--no-session"];

/// Normalize Pi profile / `/agent pi` tokens: drop unsupported flags and `--no-session`.
pub(crate) fn strip_pi_cli_args(args: &str) -> String {
    strip_unsupported_default_args(args)
        .split_whitespace()
        .filter(|token| !PI_STRIPPED_CLI_ARGS.contains(token))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Filter a token list the same way as [`strip_pi_cli_args`].
pub(crate) fn filter_pi_cli_tokens(tokens: &[String]) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }
    strip_pi_cli_args(&tokens.join(" "))
        .split_whitespace()
        .map(String::from)
        .collect()
}

/// Flags meant for Cursor/Claude CLIs that break other providers.
pub(crate) fn strip_unsupported_default_args(args: &str) -> String {
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
        .filter(|token| !UNSUPPORTED.contains(token))
        .collect();
    kept.join(" ")
}

impl AgentProfiles {
    /// Manual assembly of profiles (primarily used for testing and constructors like [`default_agent_profiles`]).
    pub(crate) fn from_parts(
        default: AgentProvider,
        providers: BTreeMap<String, AgentProviderConfig>,
    ) -> Self {
        Self { default, providers }
    }

    /// Looks up a profile by enum key. Use only after calling [`crate::config::agent_registry::normalize_profiles`],
    /// otherwise it will return an error due to missing entries.
    pub fn profile_for(&self, provider: &AgentProvider) -> Result<&AgentProviderConfig> {
        self.profile_by_id(&provider.to_string())
    }

    /// Gets a mutable reference by enum key; inserts default profile if it does not exist.
    pub fn profile_mut(&mut self, provider: &AgentProvider) -> &mut AgentProviderConfig {
        let id = provider.to_string();
        self.providers.entry(id).or_default()
    }

    /// Looks up a profile by string ID; returns an error if not found (hinting that the caller should normalize first).
    pub fn profile_by_id(&self, id: &str) -> Result<&AgentProviderConfig> {
        self.providers.get(id).ok_or_else(|| {
            anyhow::anyhow!(
                "missing agent profile for '{id}'; call agent_registry::normalize_profiles after load"
            )
        })
    }

    /// Gets a mutable reference by string ID; inserts default profile if it does not exist.
    pub fn profile_mut_by_id(&mut self, id: &str) -> &mut AgentProviderConfig {
        self.providers.entry(id.to_string()).or_default()
    }

    /// Queries whether a provider is enabled in the current profiles.
    pub fn is_provider_enabled(&self, provider: &AgentProvider) -> bool {
        self.profile_for(provider)
            .map(|profile| profile.enabled)
            .unwrap_or(false)
    }

    /// Calculates the "effective" configuration using the default provider (called internally by WebUI / daemon).
    pub fn effective_config(&self) -> Result<AgentConfig> {
        self.config_for_provider(None)
    }

    /// Calculates the final effective [`AgentConfig`]: registry defaults + user profile overrides + gateway-level normalization.
    ///
    /// Order:
    /// 1. Get the provider's default [`AgentConfig`] (like `cli_path` / `mode`, etc.) from the registry;
    /// 2. Override with the user's explicitly specified `permission` / `mode` from the profile;
    /// 3. Override with the user's explicitly specified `default_args` from the profile, and resolve `--yolo` semantics:
    ///    if the user has NOT explicitly specified `permission` but `default_args` contains `--yolo`,
    ///    automatically set `permission` to `allow`;
    /// 4. Finally, call `normalized()` to strip flags not supported across providers according to the provider's `default_args_policy`.
    pub fn config_for_provider(&self, provider: Option<AgentProvider>) -> Result<AgentConfig> {
        let selected = provider.unwrap_or_else(|| self.default.clone());
        let mut config = AgentConfig::default_for_provider(selected.clone());
        let profile = self.profile_for(&selected)?;
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
        Ok(config.normalized())
    }
}

/// Gateway-level `default_args` semantics: strip "gateway-exclusive aliases" (`--yolo`) from arguments passed
/// to the provider CLI, and record the semantics (whether "auto-approve tool execution" is enabled).
struct GatewayDefaultArgsSemantics {
    yolo: bool,
}

/// Parses gateway-exclusive aliases written by the user in `default_args`, returning the "stripped CLI arguments"
/// and the "semantic structure".
///
/// - `--yolo` is the gateway's unified alias for "auto-approve tool execution". Different providers implement this
///   differently behind the scenes (Claude uses `permission: allow`, Cursor uses `--yolo`, Codex uses `mode = auto`, etc.),
///   so it is not directly forwarded here. Instead, it is translated into `permission: allow` for the provider
///   backend to map on its own.
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
        self.agent
            .effective_config()
            .expect("normalized agent profiles")
    }

    /// In-memory defaults used by daemon / WebUI when `config.json` does not
    /// exist yet. Integrations stay disabled until `cc-gateway init` writes the file.
    pub fn runtime_defaults() -> Self {
        let mut config = Self::default();
        config.agent = crate::config::agent_registry::runtime_agent_profiles();
        config.platforms.qq.enabled = false;
        config.platforms.qq.app_id.clear();
        config.platforms.qq.app_secret.clear();
        config.platforms.feishu.enabled = false;
        config.platforms.feishu.app_id.clear();
        config.platforms.feishu.app_secret.clear();
        config.platforms.telegram.enabled = false;
        config.platforms.telegram.bot_token.clear();
        config
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: "${TELEGRAM_BOT_TOKEN}".to_string(),
            proxy: String::new(),
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

#[cfg(test)]
mod pi_cli_args_tests {
    use super::*;

    #[test]
    fn strip_pi_cli_args_removes_no_session_silently() {
        assert_eq!(
            strip_pi_cli_args("--no-session --provider anthropic"),
            "--provider anthropic"
        );
        assert_eq!(strip_pi_cli_args("--no-session"), "");
    }

    #[test]
    fn strip_pi_cli_args_removes_unsupported_and_no_session() {
        assert_eq!(strip_pi_cli_args("--no-session --yolo --force"), "");
    }

    #[test]
    fn pi_normalized_strips_no_session_from_profile_default_args() {
        let mut profiles = AgentProfiles::default();
        profiles.profile_mut(&AgentProvider::Pi).default_args =
            Some("--no-session --provider openai".to_string());
        profiles.profile_mut(&AgentProvider::Pi).enabled = true;
        let cfg = profiles
            .config_for_provider(Some(AgentProvider::Pi))
            .expect("normalized pi profile");
        assert!(!cfg.default_args.contains("--no-session"));
        assert!(cfg.default_args.contains("--provider"));
    }

    #[test]
    fn filter_pi_cli_tokens_strips_no_session_from_extra_args() {
        let tokens = vec![
            "--no-session".to_string(),
            "--model".to_string(),
            "gpt-4".to_string(),
        ];
        assert_eq!(
            filter_pi_cli_tokens(&tokens),
            vec!["--model".to_string(), "gpt-4".to_string()]
        );
    }
}
