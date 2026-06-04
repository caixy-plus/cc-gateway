//! Canonical list of agent providers integrated into cc-gateway.
//!
//! Used by CLI pickers, WebUI (`GET /api/agents`), and `/agent` prefix parsing so new
//! providers only need a registry entry plus the usual runtime wiring.

use serde_json::{json, Value};

use super::model::{AgentProfiles, AgentProvider, AgentProviderConfig};

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
}

pub const AGENT_PROVIDER_DEFS: &[AgentProviderDef] = &[
    AgentProviderDef {
        provider: AgentProvider::Claude,
        id: "claude",
        display_name: "claude",
        cli_binary: "claude",
        slash_aliases: &[],
    },
    AgentProviderDef {
        provider: AgentProvider::Cursor,
        id: "cursor",
        display_name: "cursor",
        cli_binary: "agent",
        slash_aliases: &[],
    },
    AgentProviderDef {
        provider: AgentProvider::Pi,
        id: "pi",
        display_name: "pi",
        cli_binary: "pi",
        slash_aliases: &[],
    },
    AgentProviderDef {
        provider: AgentProvider::OpenCode,
        id: "opencode",
        display_name: "opencode",
        cli_binary: "opencode",
        slash_aliases: &[],
    },
];

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
) -> &'a AgentProviderConfig {
    match def.provider {
        AgentProvider::Claude => &profiles.claude,
        AgentProvider::Cursor => &profiles.cursor,
        AgentProvider::Pi => &profiles.pi,
        AgentProvider::OpenCode => &profiles.opencode,
    }
}

pub fn profile_mut_for_def<'a>(
    profiles: &'a mut AgentProfiles,
    def: &AgentProviderDef,
) -> &'a mut AgentProviderConfig {
    match def.provider {
        AgentProvider::Claude => &mut profiles.claude,
        AgentProvider::Cursor => &mut profiles.cursor,
        AgentProvider::Pi => &mut profiles.pi,
        AgentProvider::OpenCode => &mut profiles.opencode,
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
            let profile = profile_for_def(profiles, def);
            json!({
                "id": def.id,
                "display_name": def.display_name,
                "cli_binary": def.cli_binary,
                "aliases": def.slash_aliases,
                "config": provider_config_to_json(profile),
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
        assert!(profiles.claude.enabled);
        assert!(profiles.cursor.enabled);
        assert!(profiles.opencode.enabled);
    }

    #[test]
    fn init_enablement_only_selected_when_none_installed() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::OpenCode, |_| false);
        assert!(!profiles.claude.enabled);
        assert!(!profiles.cursor.enabled);
        assert!(profiles.opencode.enabled);
    }

    #[test]
    fn init_enablement_installed_plus_selected_uninstalled() {
        let mut profiles = AgentProfiles::default();
        apply_init_agent_enablement(&mut profiles, AgentProvider::Pi, |bin| {
            matches!(bin, "claude" | "agent")
        });
        assert!(profiles.claude.enabled);
        assert!(profiles.cursor.enabled);
        assert!(profiles.pi.enabled);
        assert!(!profiles.opencode.enabled);
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
}
