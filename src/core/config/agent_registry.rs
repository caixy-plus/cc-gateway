//! Canonical list of agent providers integrated into cc-gateway.
//!
//! Used by CLI pickers, WebUI (`GET /api/agents`), and `/agent` prefix parsing so new
//! providers only need a registry entry plus the usual runtime wiring.

use std::collections::BTreeMap;

use anyhow::Result;
use serde_json::{json, Value};

use super::model::{AgentProfiles, AgentProvider, AgentProviderConfig};

/// How a provider can receive gateway MCP `send_file` context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMcpSupport {
    /// Claude Code `--mcp-config` JSON file.
    ClaudeMcpConfig,
    /// ACP `session/new` / `session/load` `mcpServers` array (OpenCode).
    AcpSession,
    /// Project-level `mcp.json` under `.cursor/` or `.pi/` (Cursor, Pi).
    ProjectMcpJson,
}

/// How profile `default_args` are normalized for a provider CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultArgsPolicy {
    /// Claude defaults only (clear wrong cross-provider permission flag).
    Claude,
    /// Strip Cursor/Claude-only tokens (OpenCode).
    StripUnsupported,
    /// Strip Pi-only tokens such as `--no-session`.
    StripPi,
    /// No provider-specific stripping beyond the global Claude-default guard.
    Passthrough,
}

/// How `/models` discovers available model ids for a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListModelsSource {
    /// Vendor-tied models (Claude, Cursor) — `/models` is rejected.
    NotSupported,
    /// Official CLI subcommand (e.g. `opencode models`).
    CliSubcommand,
    /// In-session RPC (`get_available_models` on Pi).
    InSessionRpc,
}

/// Single source of truth for provider feature flags (commands, runtime, MCP).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCapabilities {
    pub session_resume: bool,
    pub context_compact: bool,
    /// `/compact` is implemented by sending `/compact` as a user message (Claude).
    pub compact_via_user_message: bool,
    pub memory_init: bool,
    pub platform_bound: bool,
    pub list_models: ListModelsSource,
    pub in_session_model_switch: bool,
    /// `active_model_id()` is read from the live session (Pi `get_state`).
    pub active_model_from_session: bool,
    pub default_args_policy: DefaultArgsPolicy,
    pub mcp: ProviderMcpSupport,
    /// Restart/history reload shows "started" + Pi hint instead of "resumed".
    pub restart_shows_fresh_hint: bool,
    /// `/esc` and `/stop` "already idle" copy uses Claude-specific i18n keys.
    pub uses_claude_idle_copy: bool,
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
    };
}

#[derive(Debug, Clone)]
pub struct AgentProviderDef {
    pub provider: AgentProvider,
    /// `config.json` key and primary id (`AgentProvider::to_string()`).
    pub id: &'static str,
    /// Short label in `/agents` pickers and WebUI.
    pub display_name: &'static str,
    /// Binary checked by `cc-gateway init` / wizard (`which` on PATH).
    pub cli_binary: &'static str,
    /// Extra `/agent <alias>` tokens beyond `id`.
    pub slash_aliases: &'static [&'static str],
    pub capabilities: AgentCapabilities,
}

pub const AGENT_PROVIDER_DEFS: &[AgentProviderDef] = &[
    AgentProviderDef {
        provider: AgentProvider::Claude,
        id: "claude",
        display_name: "claude",
        cli_binary: "claude",
        slash_aliases: &[],
        capabilities: AgentCapabilities::CLAUDE,
    },
    AgentProviderDef {
        provider: AgentProvider::Cursor,
        id: "cursor",
        display_name: "cursor",
        cli_binary: "agent",
        slash_aliases: &[],
        capabilities: AgentCapabilities::CURSOR,
    },
    AgentProviderDef {
        provider: AgentProvider::Pi,
        id: "pi",
        display_name: "pi",
        cli_binary: "pi",
        slash_aliases: &[],
        capabilities: AgentCapabilities::PI,
    },
    AgentProviderDef {
        provider: AgentProvider::OpenCode,
        id: "opencode",
        display_name: "opencode",
        cli_binary: "opencode",
        slash_aliases: &[],
        capabilities: AgentCapabilities::OPENCODE,
    },
];

pub fn capabilities_for(provider: &AgentProvider) -> &'static AgentCapabilities {
    &def_for_provider(provider.clone()).capabilities
}

/// Reject unknown keys under `config.json` → `"agent"` / `"agent.providers"`.
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

fn registered_agent_profile_ids() -> Vec<&'static str> {
    AGENT_PROVIDER_DEFS.iter().map(|d| d.id).collect()
}

/// Normalize `default_args` for a provider using the registry policy.
pub fn normalize_default_args_for_provider(
    provider: &AgentProvider,
    default_args: &str,
) -> String {
    use super::model::{strip_pi_cli_args, AgentProvider};

    let caps = capabilities_for(provider);
    let mut args = default_args.to_string();
    if !matches!(provider, AgentProvider::Claude)
        && args == "--dangerously-skip-permissions"
    {
        args.clear();
    }
    match caps.default_args_policy {
        DefaultArgsPolicy::Claude | DefaultArgsPolicy::Passthrough => args,
        DefaultArgsPolicy::StripPi => strip_pi_cli_args(&args),
        DefaultArgsPolicy::StripUnsupported => super::model::strip_unsupported_default_args(&args),
    }
}

pub fn def_for_provider(provider: AgentProvider) -> &'static AgentProviderDef {
    AGENT_PROVIDER_DEFS
        .iter()
        .find(|d| d.provider == provider)
        .expect("every AgentProvider variant must be registered")
}

pub fn def_by_id(id: &str) -> Option<&'static AgentProviderDef> {
    let key = id.trim().to_ascii_lowercase();
    AGENT_PROVIDER_DEFS
        .iter()
        .find(|d| d.id == key || d.slash_aliases.contains(&key.as_str()))
}

pub fn parse_provider_id(token: &str) -> Option<AgentProvider> {
    def_by_id(token).map(|d| d.provider.clone())
}

pub fn profile_for_def<'a>(
    profiles: &'a AgentProfiles,
    def: &AgentProviderDef,
) -> Result<&'a AgentProviderConfig> {
    profiles.profile_by_id(def.id)
}

pub fn profile_mut_for_def<'a>(
    profiles: &'a mut AgentProfiles,
    def: &AgentProviderDef,
) -> &'a mut AgentProviderConfig {
    profiles.profile_mut_by_id(def.id)
}

/// Default profiles with one entry per registered provider id.
pub fn default_agent_profiles() -> AgentProfiles {
    let mut providers = BTreeMap::new();
    for def in AGENT_PROVIDER_DEFS {
        providers.insert(def.id.to_string(), AgentProviderConfig::default());
    }
    AgentProfiles::from_parts(AgentProvider::Claude, providers)
}

/// Ensure every registered provider id exists (after load or registry extension).
pub fn normalize_profiles(mut profiles: AgentProfiles) -> AgentProfiles {
    for def in AGENT_PROVIDER_DEFS {
        profiles.profile_mut_by_id(def.id);
    }
    profiles
}

/// Daemon / pre-init defaults: all registered providers disabled.
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

pub fn provider_config_to_json(cfg: &AgentProviderConfig) -> Value {
    json!({
        "enabled": cfg.enabled,
        "default_args": cfg.default_args,
        "mode": cfg.mode,
        "permission": cfg.permission,
    })
}

/// WebUI / API catalog: integrated providers with current profile settings.
pub fn build_agents_api_response(profiles: &AgentProfiles) -> Value {
    let providers: Vec<Value> = AGENT_PROVIDER_DEFS
        .iter()
        .map(|def| {
            let profile = profile_for_def(profiles, def)
                .cloned()
                .unwrap_or_default();
            json!({
                "id": def.id,
                "display_name": def.display_name,
                "cli_binary": def.cli_binary,
                "aliases": def.slash_aliases,
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
        assert_eq!(AGENT_PROVIDER_DEFS.len(), 4);
        for def in AGENT_PROVIDER_DEFS {
            assert_eq!(def_for_provider(def.provider.clone()).id, def.id);
        }
    }

    #[test]
    fn parse_opencode_id() {
        assert_eq!(parse_provider_id("opencode"), Some(AgentProvider::OpenCode));
    }

    #[test]
    fn init_enablement_enables_all_installed() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::Claude, |_| true);
        assert!(profiles.profile_for(&AgentProvider::Claude).unwrap().enabled);
        assert!(profiles.profile_for(&AgentProvider::Cursor).unwrap().enabled);
        assert!(profiles.profile_for(&AgentProvider::OpenCode).unwrap().enabled);
    }

    #[test]
    fn init_enablement_only_selected_when_none_installed() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::OpenCode, |_| false);
        assert!(!profiles.profile_for(&AgentProvider::Claude).unwrap().enabled);
        assert!(!profiles.profile_for(&AgentProvider::Cursor).unwrap().enabled);
        assert!(profiles.profile_for(&AgentProvider::OpenCode).unwrap().enabled);
    }

    #[test]
    fn init_enablement_installed_plus_selected_uninstalled() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::Pi, |bin| {
            matches!(bin, "claude" | "agent")
        });
        assert!(profiles.profile_for(&AgentProvider::Claude).unwrap().enabled);
        assert!(profiles.profile_for(&AgentProvider::Cursor).unwrap().enabled);
        assert!(profiles.profile_for(&AgentProvider::Pi).unwrap().enabled);
        assert!(!profiles.profile_for(&AgentProvider::OpenCode).unwrap().enabled);
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
        assert!(normalized.profile_for(&AgentProvider::Claude).unwrap().enabled);
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
            BTreeMap::from([(
                "claude".to_string(),
                AgentProviderConfig::default(),
            )]),
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
        assert!(!normalized.profile_for(&AgentProvider::Claude).unwrap().enabled);
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
