//! Agent provider registry (canonical manifest).
//!
//! # Purpose
//!
//! This module describes which providers are integrated into the gateway and the capability matrix for each.
//! All code requiring branching by provider (like `/models`, MCP injection, `/compact`, session resume,
//! `default_args` normalization, WebUI `/api/agents`, init wizard) reads from [`AGENT_PROVIDER_DEFS`] here,
//! rather than hardcoding the provider list in business logic.
//!
//! Steps to add a new provider:
//!
//! 1. Add a variant in [`super::model::AgentProvider`];
//! 2. Add a set of capability constants in [`AgentCapabilities`] (e.g., `QODER`) in this module;
//! 3. Append an [`AgentProviderDef`] entry to [`AGENT_PROVIDER_DEFS`];
//! 4. Implement the corresponding backend (e.g., [`crate::core::agent::qoder_acp::QoderAcpSession`]),
//!    and integrate it into [`crate::core::agent::session::AgentRuntime`] and the `dispatch_agent_backend!` macro.
//!
//! See § User-facing documentation in [`CLAUDE.md`](../../../../../CLAUDE.md) for document synchronization.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{json, Value};

use super::model::{AgentProfiles, AgentProvider, AgentProviderConfig};

/// The way a provider supports gateway MCP (e.g., `send_file`).
///
/// Different providers expose the MCP server provided by the gateway to their CLI through different paths:
///
/// - [`ProviderMcpSupport::ClaudeMcpConfig`]: Claude uses the `--mcp-config` JSON file;
/// - [`ProviderMcpSupport::AcpSession`]: ACP providers use the `mcpServers` field in `session/new`;
/// - [`ProviderMcpSupport::ProjectMcpJson`]: Cursor / Pi use the project-level `.cursor/mcp.json`
///   / `.pi/mcp.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMcpSupport {
    /// Claude Code `--mcp-config` JSON file.
    ClaudeMcpConfig,
    /// ACP `session/new` / `session/load` `mcpServers` array (OpenCode / Kimi /
    /// Gemini / Codex / Qoder).
    AcpSession,
    /// Project-level `mcp.json` (Cursor, Pi).
    ProjectMcpJson,
}

/// How default_args in the profile are normalized before being passed to the provider CLI.
///
/// Different providers have different tolerances for flags from other providers—for example, passing `--yolo`
/// (exclusive to Claude/Cursor) as-is to OpenCode / Kimi will trigger an error, and needs to be stripped by the
/// gateway before passing to the CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultArgsPolicy {
    /// Claude-exclusive default (only cleans up incorrect cross-provider permission flags).
    Claude,
    /// Strips Cursor / Claude-exclusive tokens (OpenCode / Kimi / Gemini / Codex / Qoder).
    StripUnsupported,
    /// Strips Pi-exclusive tokens (such as `--no-session`).
    StripPi,
    /// No provider-level stripping, only keeps the global Claude default guards (used by Cursor).
    Passthrough,
}

/// How `/models` discovers available model IDs for the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListModelsSource {
    /// Bound to a specific vendor, does not support the `/models` command (Claude / Cursor).
    NotSupported,
    /// Calls the official CLI subcommand (e.g., `opencode models`).
    CliSubcommand,
    /// In-session RPC (Pi's `get_available_models`).
    InSessionRpc,
}

/// Capabilities flags for a single provider—single source of truth.
///
/// All checks on whether "a provider supports X" must go through this struct. Do NOT hardcode provider
/// checks like `if provider == AgentProvider::Foo { … }` in business logic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCapabilities {
    /// Whether the session can be resumed after gateway restart using the persisted `provider_session_id`.
    pub session_resume: bool,
    /// Whether `/compact` is supported.
    pub context_compact: bool,
    /// Whether `/compact` is implemented by sending "/compact" as a user message (Claude-exclusive).
    pub compact_via_user_message: bool,
    /// Whether `/memory` initialization is supported (Claude-exclusive).
    pub memory_init: bool,
    /// Whether the provider is tied to a specific chat platform (currently only Claude / Cursor are restricted to
    /// "only usable on one of Feishu / Telegram / QQ"; other providers are platform-independent).
    pub platform_bound: bool,
    /// Discovery mechanism used by `/models`.
    pub list_models: ListModelsSource,
    /// Whether the model can be switched in-session (called after selected in `/models`).
    pub in_session_model_switch: bool,
    /// Whether the currently active model is read from the session state (Pi's `get_state`).
    pub active_model_from_session: bool,
    /// `default_args` normalization policy.
    pub default_args_policy: DefaultArgsPolicy,
    /// MCP injection method.
    pub mcp: ProviderMcpSupport,
    /// Whether to display a "started" hint on restart / history reload + Pi-specific hint (Pi cannot resume sessions,
    /// so every restart is treated as a fresh start displaying a specific message).
    pub restart_shows_fresh_hint: bool,
    /// Whether `/esc` and `/stop` use Claude-exclusive messages when already idle (only Claude requires special copy,
    /// other providers use the generic copy).
    pub uses_claude_idle_copy: bool,
    /// CLI tokens to inject when the gateway-level `--yolo` alias has been
    /// stripped and the final `permission` resolves to `allow`.
    ///
    /// Each provider has its own "auto-approve tools" flag:
    ///
    /// | Provider     | flag / mechanism                                            |
    /// |--------------|-----------------------------------------------------------|
    /// | Claude       | `--dangerously-skip-permissions` (kept verbatim, no injection here) |
    /// | Qoder CLI CN | `--permission-mode bypass_permissions`                    |
    /// | Others       | `&[]` (no injection; provider default / gateway-level allow handles it) |
    ///
    /// The gateway-level `--yolo` alias has already been stripped by
    /// `parse_gateway_default_args` and mapped to `permission: allow`.
    /// This field decides how that semantic is re-applied to the spawned CLI.
    pub yolo_cli_tokens: &'static [&'static str],
}

impl AgentCapabilities {
    pub const CLAUDE: Self = Self {
        session_resume: true,
        context_compact: true,
        compact_via_user_message: true,
        memory_init: true,
        platform_bound: true,
        list_models: ListModelsSource::NotSupported,
        in_session_model_switch: false,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::Claude,
        mcp: ProviderMcpSupport::ClaudeMcpConfig,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: true,
        yolo_cli_tokens: &[],
    };

    pub const CURSOR: Self = Self {
        session_resume: true,
        context_compact: false,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: true,
        list_models: ListModelsSource::NotSupported,
        in_session_model_switch: false,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::Passthrough,
        mcp: ProviderMcpSupport::ProjectMcpJson,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: false,
        yolo_cli_tokens: &[],
    };

    pub const PI: Self = Self {
        session_resume: false,
        context_compact: true,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: false,
        list_models: ListModelsSource::InSessionRpc,
        in_session_model_switch: true,
        active_model_from_session: true,
        default_args_policy: DefaultArgsPolicy::StripPi,
        mcp: ProviderMcpSupport::ProjectMcpJson,
        restart_shows_fresh_hint: true,
        uses_claude_idle_copy: false,
        yolo_cli_tokens: &[],
    };

    pub const OPENCODE: Self = Self {
        session_resume: true,
        context_compact: false,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: false,
        list_models: ListModelsSource::CliSubcommand,
        in_session_model_switch: true,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::StripUnsupported,
        mcp: ProviderMcpSupport::AcpSession,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: false,
        yolo_cli_tokens: &[],
    };

    pub const KIMI: Self = Self {
        session_resume: true,
        context_compact: false,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: false,
        list_models: ListModelsSource::InSessionRpc,
        in_session_model_switch: true,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::StripUnsupported,
        mcp: ProviderMcpSupport::AcpSession,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: false,
        // `kimi --yolo acp` (root-level flag before the `acp` subcommand).
        // Verified: `kimi --help` documents `-y, --yolo  Automatically approve all actions`.
        // In ACP mode kimi currently emits 0 `session/request_permission` notifications,
        // so this flag is redundant at runtime, but we forward it when the user sets `--yolo`
        // so the CLI behaves as documented if that ever changes.
        yolo_cli_tokens: &["--yolo"],
    };

    pub const GEMINI: Self = Self {
        session_resume: true,
        context_compact: false,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: false,
        list_models: ListModelsSource::InSessionRpc,
        in_session_model_switch: true,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::StripUnsupported,
        mcp: ProviderMcpSupport::AcpSession,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: false,
        yolo_cli_tokens: &[],
    };

    pub const CODEX: Self = Self {
        session_resume: true,
        context_compact: false,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: false,
        list_models: ListModelsSource::InSessionRpc,
        in_session_model_switch: true,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::StripUnsupported,
        mcp: ProviderMcpSupport::AcpSession,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: false,
        yolo_cli_tokens: &[],
    };

    pub const QODER: Self = Self {
        session_resume: true,
        context_compact: false,
        compact_via_user_message: false,
        memory_init: false,
        platform_bound: false,
        list_models: ListModelsSource::InSessionRpc,
        in_session_model_switch: true,
        active_model_from_session: false,
        default_args_policy: DefaultArgsPolicy::StripUnsupported,
        mcp: ProviderMcpSupport::AcpSession,
        restart_shows_fresh_hint: false,
        uses_claude_idle_copy: false,
        // qoderclicn does NOT accept `--yolo`; its real "auto-approve everything"
        // flag is `--permission-mode bypass_permissions`. Verified locally:
        // with this flag set, qoderclicn in ACP mode emits **zero**
        // `session/request_permission` notifications for file writes and shell
        // commands.
        yolo_cli_tokens: &["--permission-mode", "bypass_permissions"],
    };
}

/// Registration metadata for a single provider.
///
/// Combined with [`AgentCapabilities`], this constitutes the gateway's complete knowledge of the provider.
/// Note that `id` must be identical to [`super::model::AgentProvider::to_string()`], otherwise `config.json`
/// parsing will fail to find the provider.
#[derive(Debug, Clone)]
pub struct AgentProviderDef {
    pub provider: AgentProvider,
    /// The key in `config.json`, which is also returned by [`AgentProvider::to_string()`].
    pub id: &'static str,
    /// Short label displayed in the `/agents` list and WebUI.
    pub display_name: &'static str,
    /// Binary name checked with `which` in `cc-gateway init` / wizard.
    pub cli_binary: &'static str,
    /// Additional aliases for the `/agent <alias>` command besides `id` (empty for most providers).
    pub slash_aliases: &'static [&'static str],
    /// Optional `wizard.*` i18n key—when `cli_binary` is not on PATH, the init wizard displays this install hint.
    pub install_hint_key: Option<&'static str>,
    /// Quick `default_args` chips shown to users in the WebUI settings panel.
    ///
    /// The gateway sends this list to the frontend via `GET /api/agents`, which renders them as clickable tags.
    /// New providers get their chips without modifying frontend code.
    pub default_args_suggestions: &'static [&'static str],
    /// Capability flags.
    pub capabilities: AgentCapabilities,
}

/// **Display order** of registered providers.
///
/// The array order directly determines:
///
/// - The order of appearance in the `/agents` list;
/// - The order of provider cards in the WebUI Settings panel;
/// - The order of CLI checks in the `cc-gateway init` wizard.
///
/// When adding a new provider, insert it alphabetically or by historical integration order. Do NOT change the existing
/// relative order, otherwise it will break the `registry_display_order` unit test and WebUI visual consistency.
pub const AGENT_PROVIDER_DEFS: &[AgentProviderDef] = &[
    AgentProviderDef {
        provider: AgentProvider::Claude,
        id: "claude",
        display_name: "claude",
        cli_binary: "claude",
        slash_aliases: &[],
        install_hint_key: None,
        default_args_suggestions: &["--dangerously-skip-permissions", "--yolo"],
        capabilities: AgentCapabilities::CLAUDE,
    },
    AgentProviderDef {
        provider: AgentProvider::Codex,
        id: "codex",
        display_name: "codex",
        // Zed's ACP adapter for the Codex CLI: `npm i -g @zed-industries/codex-acp`.
        cli_binary: "codex-acp",
        slash_aliases: &[],
        install_hint_key: Some("wizard.install_hint_codex"),
        // codex-acp has no `--yolo` CLI flag. Gateway `--yolo` maps to
        // `permission: allow` + `session/set_mode full-access` in `codex_acp.rs`.
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::CODEX,
    },
    AgentProviderDef {
        provider: AgentProvider::Cursor,
        id: "cursor",
        display_name: "cursor",
        cli_binary: "agent",
        slash_aliases: &[],
        install_hint_key: None,
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::CURSOR,
    },
    AgentProviderDef {
        provider: AgentProvider::OpenCode,
        id: "opencode",
        display_name: "opencode",
        cli_binary: "opencode",
        slash_aliases: &[],
        install_hint_key: None,
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::OPENCODE,
    },
    AgentProviderDef {
        provider: AgentProvider::Kimi,
        id: "kimi",
        display_name: "kimi",
        cli_binary: "kimi",
        slash_aliases: &[],
        install_hint_key: None,
        // Root-level flag, placed before the `acp` subcommand: `kimi --yolo acp`.
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::KIMI,
    },
    AgentProviderDef {
        provider: AgentProvider::Gemini,
        id: "gemini",
        display_name: "gemini",
        cli_binary: "gemini",
        slash_aliases: &[],
        install_hint_key: None,
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::GEMINI,
    },
    AgentProviderDef {
        provider: AgentProvider::Qoder,
        id: "qoder",
        display_name: "qoder",
        cli_binary: "qoderclicn",
        slash_aliases: &[],
        install_hint_key: None,
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::QODER,
    },
    AgentProviderDef {
        provider: AgentProvider::Pi,
        id: "pi",
        display_name: "pi",
        cli_binary: "pi",
        slash_aliases: &[],
        install_hint_key: None,
        default_args_suggestions: &["--yolo"],
        capabilities: AgentCapabilities::PI,
    },
];

/// Queries the capability flags of the specified provider.
///
/// Returns a `'static` reference, which callers can safely `Copy`.
/// Panics if the provider is not registered in [`AGENT_PROVIDER_DEFS`] (to catch development errors early).
pub fn capabilities_for(provider: &AgentProvider) -> &'static AgentCapabilities {
    &def_for_provider(provider.clone()).capabilities
}

/// Validates whether unknown keys appear under the `agent` and `agent.providers` fields in `config.json`.
///
/// - The top-level `agent.*` only allows `default` and `providers` (old flat keys have been migrated by
///   `upgrade_config_json`, so any remaining ones are typos);
/// - The keys under `agent.providers.*` must find a corresponding id in [`AGENT_PROVIDER_DEFS`].
///
/// Returns an error with a clear message if validation fails, enabling the loader to prompt the user for corrections.
pub fn validate_agent_profile_keys(value: &Value) -> Result<()> {
    let Some(agent) = value.get("agent").and_then(|v| v.as_object()) else {
        return Ok(());
    };
    for key in agent.keys() {
        if key == "default" || key == "providers" {
            continue;
        }
        anyhow::bail!(
            "unknown key '{key}' in config.json \"agent\" section; \
             expected only \"default\" and \"providers\""
        );
    }
    let Some(providers) = agent.get("providers").and_then(|v| v.as_object()) else {
        return Ok(());
    };
    for key in providers.keys() {
        if def_by_id(key).is_none() {
            anyhow::bail!(
                "unknown agent profile key '{key}' in config.json \"agent.providers\"; \
                 expected one of: {}",
                registered_agent_profile_ids().join(", ")
            );
        }
    }
    Ok(())
}

/// Returns a list of IDs of all registered providers (used for error messages).
fn registered_agent_profile_ids() -> Vec<&'static str> {
    AGENT_PROVIDER_DEFS.iter().map(|d| d.id).collect()
}

/// Normalizes the profile's `default_args` according to the provider's [`DefaultArgsPolicy`].
///
/// - If a non-Claude provider has a value exactly equal to `--dangerously-skip-permissions`, it is cleared immediately
///   (this is Claude's "skip all permissions" switch, which is invalid and dangerous for other providers);
/// - Fine-grained stripping is then executed according to the provider's policy (Pi / StripUnsupported / Passthrough).
pub fn normalize_default_args_for_provider(provider: &AgentProvider, default_args: &str) -> String {
    use super::model::{strip_pi_cli_args, AgentProvider};

    let caps = capabilities_for(provider);
    let mut args = default_args.to_string();
    if !matches!(provider, AgentProvider::Claude) && args == "--dangerously-skip-permissions" {
        args.clear();
    }
    match caps.default_args_policy {
        DefaultArgsPolicy::Claude | DefaultArgsPolicy::Passthrough => args,
        DefaultArgsPolicy::StripPi => strip_pi_cli_args(&args),
        DefaultArgsPolicy::StripUnsupported => super::model::strip_unsupported_default_args(&args),
    }
}

/// Finds the corresponding registration entry using the [`AgentProvider`] enum.
pub fn def_for_provider(provider: AgentProvider) -> &'static AgentProviderDef {
    AGENT_PROVIDER_DEFS
        .iter()
        .find(|d| d.provider == provider)
        .expect("every AgentProvider variant must be registered")
}

/// Finds the corresponding registration entry by ID (lowercase, optional spaces) or alias; returns `None` if not found.
pub fn def_by_id(id: &str) -> Option<&'static AgentProviderDef> {
    let key = id.trim().to_ascii_lowercase();
    AGENT_PROVIDER_DEFS
        .iter()
        .find(|d| d.id == key || d.slash_aliases.contains(&key.as_str()))
}

/// Parses the provider token entered by the user in the chat (e.g., `/agent qoder`, `/agent claude`).
///
/// Supports both primary ID and `slash_aliases`. Unknown tokens return `None` for the caller to handle gracefully.
pub fn parse_provider_id(token: &str) -> Option<AgentProvider> {
    def_by_id(token).map(|d| d.provider.clone())
}

/// Locates the corresponding profile in the profiles using [`AgentProviderDef`].
pub fn profile_for_def<'a>(
    profiles: &'a AgentProfiles,
    def: &AgentProviderDef,
) -> Result<&'a AgentProviderConfig> {
    profiles.profile_by_id(def.id)
}

/// Locates the corresponding profile in the profiles using [`AgentProviderDef`] and gets a mutable reference.
pub fn profile_mut_for_def<'a>(
    profiles: &'a mut AgentProfiles,
    def: &AgentProviderDef,
) -> &'a mut AgentProviderConfig {
    profiles.profile_mut_by_id(def.id)
}

/// Default profiles: each registered provider id has a default profile;
/// `default` defaults to `claude`.
pub fn default_agent_profiles() -> AgentProfiles {
    let mut providers = BTreeMap::new();
    for def in AGENT_PROVIDER_DEFS {
        providers.insert(def.id.to_string(), AgentProviderConfig::default());
    }
    AgentProfiles::from_parts(AgentProvider::Claude, providers)
}

/// Normalizes profiles: ensures every registered provider id has an entry in the `providers` map.
///
/// Used after `config.json` is loaded—if an old file lacks a newly added provider (e.g., `qoder`),
/// calling this function fills it in, so downstream code can safely call `profile_by_id(id)` without handling `None`.
pub fn normalize_profiles(mut profiles: AgentProfiles) -> AgentProfiles {
    for def in AGENT_PROVIDER_DEFS {
        profiles.profile_mut_by_id(def.id);
    }
    profiles
}

/// In-memory default values before daemon start / first `init`: all registered providers have `enabled = false`,
/// preventing the gateway from automatically enabling a CLI without the user's explicit choice.
pub fn runtime_agent_profiles() -> AgentProfiles {
    let mut profiles = default_agent_profiles();
    for def in AGENT_PROVIDER_DEFS {
        profile_mut_for_def(&mut profiles, def).enabled = false;
    }
    profiles
}

impl Default for AgentProfiles {
    fn default() -> Self {
        default_agent_profiles()
    }
}

/// Init wizard: enable every installed CLI; if the user picks an uninstalled default,
/// enable only that provider among the not-installed set (others stay disabled).
pub fn apply_init_agent_enablement(
    profiles: &mut AgentProfiles,
    default: AgentProvider,
    is_installed: impl Fn(&str) -> bool,
) {
    for def in AGENT_PROVIDER_DEFS {
        let installed = is_installed(def.cli_binary);
        let selected = def.provider == default;
        let enabled = installed || selected;
        profile_mut_for_def(profiles, def).enabled = enabled;
    }
}

/// Serializes a profile into the shape of a single provider object output by `GET /api/agents`.
pub fn provider_config_to_json(cfg: &AgentProviderConfig) -> Value {
    json!({
        "enabled": cfg.enabled,
        "default_args": cfg.default_args,
        "mode": cfg.mode,
        "permission": cfg.permission,
    })
}

/// Constructs the complete response body for `GET /api/agents`: sends all registered providers along with current profile
/// configurations down to the WebUI.
///
/// The frontend can render the provider card list with this JSON: id, display_name, cli_binary,
/// optional `default_args` suggestions (chips), and the user's current `enabled` / `default_args` / `mode`
/// / `permission`.
///
/// Compatible with both "flat key" and "nested canonical" shapes in WebUI `POST /api/config`:
/// Older WebUI versions place edited profiles under the `agent` object on the same level as `providers`
/// (e.g., `{ "default": "claude", "providers": {...}, "claude": {...} }`).
/// Here, flat keys are preferred (as they represent the user's latest edits in the UI); unknown keys will trigger errors.
pub fn agent_profiles_from_api_json(value: &Value) -> Result<AgentProfiles> {
    let obj = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("\"agent\" must be a JSON object"))?;
    let mut profiles: AgentProfiles = serde_json::from_value(value.clone())
        .map_err(|e| anyhow::anyhow!("invalid \"agent\" section: {e}"))?;
    for (key, profile_value) in obj {
        if key == "default" || key == "providers" {
            continue;
        }
        if def_by_id(key).is_none() {
            anyhow::bail!("unknown agent provider key '{key}' in \"agent\" section");
        }
        let parsed: AgentProviderConfig = serde_json::from_value(profile_value.clone())
            .map_err(|e| anyhow::anyhow!("invalid profile for agent '{key}': {e}"))?;
        profiles.providers.insert(key.clone(), parsed);
    }
    Ok(profiles)
}

/// Constructs the `GET /api/agents` response body: bundles and returns all registered provider IDs, display names,
/// default CLIs, `default_args` suggestions, and current profiles.
pub fn build_agents_api_response(profiles: &AgentProfiles) -> Value {
    let providers: Vec<Value> = AGENT_PROVIDER_DEFS
        .iter()
        .map(|def| {
            let profile = profile_for_def(profiles, def).cloned().unwrap_or_default();
            json!({
                "id": def.id,
                "display_name": def.display_name,
                "cli_binary": def.cli_binary,
                "aliases": def.slash_aliases,
                "default_args_suggestions": def.default_args_suggestions,
                "config": provider_config_to_json(&profile),
            })
        })
        .collect();

    json!({
        "default": profiles.default.to_string(),
        "providers": providers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lists_all_provider_variants() {
        assert_eq!(AGENT_PROVIDER_DEFS.len(), 8);
        for def in AGENT_PROVIDER_DEFS {
            assert_eq!(def_for_provider(def.provider.clone()).id, def.id);
        }
    }

    #[test]
    fn parse_qoder_id() {
        assert_eq!(parse_provider_id("qoder"), Some(AgentProvider::Qoder));
    }

    #[test]
    fn parse_codex_id() {
        assert_eq!(parse_provider_id("codex"), Some(AgentProvider::Codex));
    }

    #[test]
    fn codex_registry_has_install_hint() {
        let def = def_for_provider(AgentProvider::Codex);
        assert_eq!(def.cli_binary, "codex-acp");
        assert_eq!(def.install_hint_key, Some("wizard.install_hint_codex"));
    }

    #[test]
    fn parse_opencode_id() {
        assert_eq!(parse_provider_id("opencode"), Some(AgentProvider::OpenCode));
    }

    #[test]
    fn registry_display_order() {
        let ids: Vec<_> = AGENT_PROVIDER_DEFS.iter().map(|d| d.id).collect();
        assert_eq!(
            ids,
            vec!["claude", "codex", "cursor", "opencode", "kimi", "gemini", "qoder", "pi"]
        );
    }

    #[test]
    fn parse_kimi_id() {
        assert_eq!(parse_provider_id("kimi"), Some(AgentProvider::Kimi));
    }

    #[test]
    fn parse_gemini_id() {
        assert_eq!(parse_provider_id("gemini"), Some(AgentProvider::Gemini));
    }

    #[test]
    fn init_enablement_enables_all_installed() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::Claude, |_| true);
        assert!(
            profiles
                .profile_for(&AgentProvider::Claude)
                .unwrap()
                .enabled
        );
        assert!(
            profiles
                .profile_for(&AgentProvider::Cursor)
                .unwrap()
                .enabled
        );
        assert!(
            profiles
                .profile_for(&AgentProvider::OpenCode)
                .unwrap()
                .enabled
        );
        assert!(profiles.profile_for(&AgentProvider::Kimi).unwrap().enabled);
        assert!(
            profiles
                .profile_for(&AgentProvider::Gemini)
                .unwrap()
                .enabled
        );
        assert!(profiles.profile_for(&AgentProvider::Qoder).unwrap().enabled);
    }

    #[test]
    fn init_enablement_only_selected_when_none_installed() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::OpenCode, |_| false);
        assert!(
            !profiles
                .profile_for(&AgentProvider::Claude)
                .unwrap()
                .enabled
        );
        assert!(
            !profiles
                .profile_for(&AgentProvider::Cursor)
                .unwrap()
                .enabled
        );
        assert!(
            profiles
                .profile_for(&AgentProvider::OpenCode)
                .unwrap()
                .enabled
        );
        assert!(!profiles.profile_for(&AgentProvider::Kimi).unwrap().enabled);
        assert!(!profiles.profile_for(&AgentProvider::Qoder).unwrap().enabled);
    }

    #[test]
    fn init_enablement_installed_plus_selected_uninstalled() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::Pi, |bin| {
            matches!(bin, "claude" | "agent")
        });
        assert!(
            profiles
                .profile_for(&AgentProvider::Claude)
                .unwrap()
                .enabled
        );
        assert!(
            profiles
                .profile_for(&AgentProvider::Cursor)
                .unwrap()
                .enabled
        );
        assert!(profiles.profile_for(&AgentProvider::Pi).unwrap().enabled);
        assert!(
            !profiles
                .profile_for(&AgentProvider::OpenCode)
                .unwrap()
                .enabled
        );
        assert!(!profiles.profile_for(&AgentProvider::Kimi).unwrap().enabled);
        assert!(!profiles.profile_for(&AgentProvider::Qoder).unwrap().enabled);
    }

    #[test]
    fn agents_api_includes_every_registered_provider() {
        let body = build_agents_api_response(&AgentProfiles::default());
        let providers = body.get("providers").unwrap().as_array().unwrap();
        assert_eq!(providers.len(), AGENT_PROVIDER_DEFS.len());
        assert!(providers
            .iter()
            .any(|p| p.get("id") == Some(&json!("opencode"))));
    }

    #[test]
    fn agent_profiles_from_api_json_flat_keys_win_over_stale_providers() {
        let value = json!({
            "default": "claude",
            "providers": { "claude": { "default_args": "old" } },
            "claude": { "default_args": "new" }
        });
        let profiles = agent_profiles_from_api_json(&value).expect("parse");
        assert_eq!(
            profiles
                .providers
                .get("claude")
                .and_then(|p| p.default_args.as_deref()),
            Some("new")
        );
    }

    #[test]
    fn agent_profiles_from_api_json_accepts_canonical_shape() {
        let value = json!({
            "default": "gemini",
            "providers": { "gemini": { "default_args": "--yolo" } }
        });
        let profiles = agent_profiles_from_api_json(&value).expect("parse");
        assert_eq!(profiles.default.to_string(), "gemini");
        assert_eq!(
            profiles
                .providers
                .get("gemini")
                .and_then(|p| p.default_args.as_deref()),
            Some("--yolo")
        );
    }

    #[test]
    fn agent_profiles_from_api_json_rejects_unknown_provider_key() {
        let value = json!({
            "default": "claude",
            "clade": { "default_args": "typo" }
        });
        assert!(agent_profiles_from_api_json(&value).is_err());
    }

    #[test]
    fn agents_api_exposes_default_args_suggestions() {
        let body = build_agents_api_response(&AgentProfiles::default());
        let providers = body.get("providers").unwrap().as_array().unwrap();
        let suggestions_of = |id: &str| {
            providers
                .iter()
                .find(|p| p.get("id") == Some(&json!(id)))
                .and_then(|p| p.get("default_args_suggestions"))
                .cloned()
                .expect("default_args_suggestions present")
        };
        assert_eq!(
            suggestions_of("claude"),
            json!(["--dangerously-skip-permissions", "--yolo"])
        );
        assert_eq!(suggestions_of("gemini"), json!(["--yolo"]));
        // Verified locally: `kimi --yolo acp` starts the ACP server normally.
        assert_eq!(suggestions_of("kimi"), json!(["--yolo"]));
        assert_eq!(suggestions_of("qoder"), json!(["--yolo"]));
        // Codex: `--yolo` is a gateway-level flag mapped to `full-access` mode;
        // no other CLI flags are suggested (codex-acp only takes `-c key=value`).
        assert_eq!(suggestions_of("codex"), json!(["--yolo"]));
    }

    #[test]
    fn agent_profiles_serde_roundtrip_preserves_providers_object() {
        let profiles = default_agent_profiles();
        let json = serde_json::to_value(&profiles).expect("serialize");
        assert_eq!(json.get("default").and_then(|v| v.as_str()), Some("claude"));
        let providers = json
            .get("providers")
            .and_then(|v| v.as_object())
            .expect("providers object");
        assert!(providers.contains_key("claude"));
        assert!(providers.contains_key("cursor"));

        let restored: AgentProfiles =
            serde_json::from_value(json).expect("deserialize nested agent profiles");
        let normalized = normalize_profiles(restored);
        assert!(
            normalized
                .profile_for(&AgentProvider::Claude)
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn validate_agent_profile_keys_rejects_unknown_top_level_agent_key() {
        let raw = json!({
            "agent": {
                "default": "claude",
                "providers": { "claude": {} },
                "legacy_flat": { "enabled": true }
            }
        });
        let err = validate_agent_profile_keys(&raw).unwrap_err();
        assert!(err.to_string().contains("legacy_flat"));
    }

    #[test]
    fn profile_for_errors_when_registry_id_missing() {
        let profiles = AgentProfiles::from_parts(
            AgentProvider::Claude,
            BTreeMap::from([("claude".to_string(), AgentProviderConfig::default())]),
        );
        assert!(profiles.profile_for(&AgentProvider::Pi).is_err());
    }

    #[test]
    fn normalize_profiles_adds_missing_registry_entries() {
        let profiles = AgentProfiles::from_parts(
            AgentProvider::Claude,
            BTreeMap::from([(
                "claude".to_string(),
                AgentProviderConfig {
                    enabled: false,
                    ..Default::default()
                },
            )]),
        );
        let normalized = normalize_profiles(profiles);
        assert!(
            !normalized
                .profile_for(&AgentProvider::Claude)
                .unwrap()
                .enabled
        );
        assert!(normalized.profile_for(&AgentProvider::Pi).unwrap().enabled);
    }

    #[test]
    fn capability_matrix_matches_integrated_providers() {
        let claude = capabilities_for(&AgentProvider::Claude);
        assert!(claude.session_resume);
        assert!(claude.context_compact);
        assert!(claude.compact_via_user_message);
        assert!(claude.memory_init);
        assert!(claude.platform_bound);
        assert!(!claude.active_model_from_session);
        assert_eq!(claude.mcp, ProviderMcpSupport::ClaudeMcpConfig);

        let pi = capabilities_for(&AgentProvider::Pi);
        assert!(!pi.session_resume);
        assert!(pi.context_compact);
        assert!(!pi.compact_via_user_message);
        assert!(pi.active_model_from_session);
        assert!(pi.restart_shows_fresh_hint);
        assert_eq!(pi.list_models, ListModelsSource::InSessionRpc);

        let opencode = capabilities_for(&AgentProvider::OpenCode);
        assert!(opencode.session_resume);
        assert!(!opencode.context_compact);
        assert!(opencode.in_session_model_switch);
        assert_eq!(opencode.list_models, ListModelsSource::CliSubcommand);
        assert_eq!(opencode.mcp, ProviderMcpSupport::AcpSession);

        let cursor = capabilities_for(&AgentProvider::Cursor);
        assert!(!cursor.context_compact);
        assert_eq!(cursor.mcp, ProviderMcpSupport::ProjectMcpJson);

        let kimi = capabilities_for(&AgentProvider::Kimi);
        assert!(kimi.session_resume);
        assert!(kimi.in_session_model_switch);
        assert_eq!(kimi.list_models, ListModelsSource::InSessionRpc);
        assert_eq!(kimi.mcp, ProviderMcpSupport::AcpSession);

        let gemini = capabilities_for(&AgentProvider::Gemini);
        assert!(gemini.session_resume);
        assert!(gemini.in_session_model_switch);
        assert_eq!(gemini.list_models, ListModelsSource::InSessionRpc);
        assert_eq!(gemini.mcp, ProviderMcpSupport::AcpSession);

        let codex = capabilities_for(&AgentProvider::Codex);
        assert!(codex.in_session_model_switch);
        assert_eq!(codex.list_models, ListModelsSource::InSessionRpc);

        let qoder = capabilities_for(&AgentProvider::Qoder);
        assert!(qoder.session_resume);
        assert!(qoder.in_session_model_switch);
        assert_eq!(qoder.list_models, ListModelsSource::InSessionRpc);
        assert_eq!(qoder.mcp, ProviderMcpSupport::AcpSession);
    }

    #[test]
    fn validate_agent_profile_keys_rejects_unknown_id() {
        let raw = json!({
            "agent": {
                "default": "claude",
                "providers": {
                    "claude": {},
                    "typo_provider": { "enabled": true }
                }
            }
        });
        let err = validate_agent_profile_keys(&raw).unwrap_err();
        assert!(err.to_string().contains("typo_provider"));
    }

    #[test]
    fn validate_agent_profile_keys_accepts_registered_ids() {
        let raw = json!({
            "agent": {
                "default": "claude",
                "providers": {
                    "claude": {},
                    "pi": {}
                }
            }
        });
        validate_agent_profile_keys(&raw).expect("registered keys");
    }

    #[test]
    fn normalize_default_args_pi_strips_no_session() {
        let out = normalize_default_args_for_provider(
            &AgentProvider::Pi,
            "--no-session --provider openai",
        );
        assert!(!out.contains("--no-session"));
        assert!(out.contains("--provider"));
    }
}
