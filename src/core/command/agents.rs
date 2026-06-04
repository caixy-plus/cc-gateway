use crate::config::model::AgentProvider;
use crate::{t, t_fmt};

/// Providers enabled in config — appear in `/agents` pickers and channel agent cards.
pub fn available_providers(profiles: &crate::config::model::AgentProfiles) -> Vec<AgentProvider> {
    crate::config::agent_registry::AGENT_PROVIDER_DEFS
        .iter()
        .filter(|def| profiles.is_provider_enabled(&def.provider))
        .map(|def| def.provider.clone())
        .collect()
}

pub fn provider_display_name(provider: &AgentProvider) -> &'static str {
    crate::config::agent_registry::def_for_provider(provider.clone()).display_name
}

/// Build picker rows: `(label, selectable)`.
pub fn build_provider_items(
    profiles: &crate::config::model::AgentProfiles,
    current_default: &AgentProvider,
) -> Vec<(String, bool)> {
    available_providers(profiles)
        .into_iter()
        .map(|provider| {
            let label = if &provider == current_default {
                t_fmt!(
                    "builtin.agent_option_default",
                    NAME = provider_display_name(&provider)
                )
            } else {
                provider_display_name(&provider).to_string()
            };
            (label, true)
        })
        .collect()
}

pub fn format_provider_picker_message(
    profiles: &crate::config::model::AgentProfiles,
    current_default: &AgentProvider,
) -> String {
    let items = build_provider_items(profiles, current_default);
    let mut out = t!("builtin.select_agent_prompt").to_string();
    for (i, (label, _)) in items.iter().enumerate() {
        out.push_str(&format!("\n  {}. {}", i + 1, label));
    }
    out.push_str(&format!("\n{}", t!("builtin.agents_pick_hint")));
    out
}

pub fn provider_at_index(
    profiles: &crate::config::model::AgentProfiles,
    idx: usize,
) -> Option<AgentProvider> {
    available_providers(profiles).into_iter().nth(idx)
}

pub fn session_started_message(provider: &AgentProvider, dir: &str) -> String {
    t_fmt!(
        "builtin.session_started",
        NAME = provider_display_name(provider),
        DIR = dir
    )
}

pub fn session_resumed_message(provider: &AgentProvider, dir: &str) -> String {
    t_fmt!(
        "builtin.session_resumed",
        NAME = provider_display_name(provider),
        DIR = dir
    )
}

/// Whether the provider CLI can reload a prior conversation (ACP load / Claude `--resume` / Pi `switch_session`).
pub fn provider_supports_session_resume(provider: &AgentProvider) -> bool {
    !matches!(provider, AgentProvider::Pi)
}

/// Message shown when a session is restarted (e.g. user clicks "Restart Session" or `/agent-history N`).
pub fn session_restarted_message(provider: &AgentProvider, dir: &str) -> String {
    if matches!(provider, AgentProvider::Pi) {
        return format!(
            "{}\n\n{}",
            session_started_message(provider, dir),
            t!("builtin.session_restarted_pi_hint")
        );
    }
    session_resumed_message(provider, dir)
}

pub fn session_stopped_message(provider: &AgentProvider) -> String {
    t_fmt!(
        "builtin.session_stopped",
        NAME = provider_display_name(provider)
    )
}

/// Map raw spawn/resume errors (often English `anyhow` text) to localized user messages.
pub fn friendly_spawn_error(raw: &str) -> String {
    if raw.contains("ACP did not return a session id") {
        return t!("agent.acp_no_session_id").to_string();
    }
    if raw.contains("ACP request timed out") {
        return t!("agent.acp_request_timeout").to_string();
    }
    if raw.contains("process exited immediately after spawn; no stderr") {
        return t!("agent.process_exited_no_stderr").to_string();
    }
    if let Some(detail) = raw
        .strip_prefix("opencode process exited immediately after spawn:")
        .or_else(|| raw.strip_prefix("cursor process exited immediately after spawn:"))
        .or_else(|| raw.strip_prefix("claude process exited immediately after spawn:"))
        .or_else(|| {
            raw.split(" process exited immediately after spawn:")
                .nth(1)
                .map(str::trim)
        })
    {
        return t_fmt!("agent.process_exited", DETAIL = detail);
    }
    if raw.contains("Failed to spawn") || raw.contains("Failed to open") {
        return t_fmt!("agent.spawn_failed", DETAIL = raw);
    }
    raw.to_string()
}

pub fn failed_start_agent_message(provider: &AgentProvider, err: impl std::fmt::Display) -> String {
    let raw = err.to_string();
    t_fmt!(
        "builtin.failed_start_agent",
        NAME = provider_display_name(provider),
        ERR = friendly_spawn_error(&raw)
    )
}

/// User-visible reply when `/esc` is used but there is nothing to flush.
pub fn esc_already_idle_message(provider: &AgentProvider) -> String {
    match provider {
        AgentProvider::Claude => t!("builtin.esc_already_idle_claude").to_string(),
        _ => t!("builtin.esc_already_idle").to_string(),
    }
}

/// User-visible reply when `/stop` is used but the agent is already idle.
pub fn stop_already_idle_message(provider: &AgentProvider) -> String {
    match provider {
        AgentProvider::Claude => t!("builtin.stop_already_idle_claude").to_string(),
        _ => t!("builtin.stop_already_idle").to_string(),
    }
}

/// User-visible reply after `/stop` succeeds.
pub fn stop_sent_message(provider: &AgentProvider) -> String {
    match provider {
        AgentProvider::Claude => t!("builtin.stop_sent_claude").to_string(),
        AgentProvider::Cursor => t!("builtin.stop_sent_cursor").to_string(),
        AgentProvider::Pi => t!("builtin.stop_sent_pi").to_string(),
        AgentProvider::OpenCode => t!("builtin.stop_sent_opencode").to_string(),
    }
}

/// User-visible reply after `/esc` without a prompt succeeds.
pub fn esc_sent_message(provider: &AgentProvider) -> String {
    match provider {
        AgentProvider::Claude => t!("builtin.esc_sent_claude").to_string(),
        AgentProvider::Cursor => t!("builtin.esc_sent_cursor").to_string(),
        AgentProvider::Pi => t!("builtin.esc_sent_pi").to_string(),
        AgentProvider::OpenCode => t!("builtin.esc_sent_opencode").to_string(),
    }
}

/// User-visible reply after `/esc <prompt>` succeeds.
pub fn esc_with_prompt_sent_message(provider: &AgentProvider, msg: &str) -> String {
    match provider {
        AgentProvider::Claude => t_fmt!("builtin.esc_with_prompt_sent_claude", MSG = msg),
        AgentProvider::Cursor => t_fmt!("builtin.esc_with_prompt_sent_cursor", MSG = msg),
        AgentProvider::Pi => t_fmt!("builtin.esc_with_prompt_sent_pi", MSG = msg),
        AgentProvider::OpenCode => t_fmt!("builtin.esc_with_prompt_sent_opencode", MSG = msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{AgentProfiles, AgentProviderConfig};

    fn test_profiles_both() -> AgentProfiles {
        AgentProfiles {
            claude: AgentProviderConfig::default(),
            cursor: AgentProviderConfig::default(),
            ..Default::default()
        }
    }

    #[test]
    fn build_provider_items_marks_current_default() {
        let profiles = test_profiles_both();
        let items = build_provider_items(&profiles, &AgentProvider::Cursor);
        // All four providers enabled by default
        assert_eq!(items.len(), 4);
        assert!(items.iter().any(|(label, _)| label.contains("claude")));
        assert!(items.iter().any(|(label, _)| label.contains("cursor")));
    }

    #[test]
    fn session_started_message_uses_provider_display_name() {
        let msg = session_started_message(&AgentProvider::Cursor, "/tmp/work");
        assert!(msg.contains("cursor"));
        assert!(!msg.contains("claude"));
    }

    #[test]
    fn session_restarted_message_for_pi_includes_resume_hint() {
        let msg = session_restarted_message(&AgentProvider::Pi, "/tmp/work");
        assert!(msg.contains(crate::t!("builtin.session_restarted_pi_hint")));
    }

    #[test]
    fn pi_does_not_support_provider_session_resume() {
        assert!(!provider_supports_session_resume(&AgentProvider::Pi));
        assert!(provider_supports_session_resume(&AgentProvider::Claude));
    }

    #[test]
    fn format_provider_picker_lists_enabled_agents() {
        let profiles = test_profiles_both();
        let msg = format_provider_picker_message(&profiles, &AgentProvider::Claude);
        assert!(msg.contains("claude"));
        assert!(msg.contains("cursor"));
    }

    #[test]
    fn available_providers_filters_by_configured_cli_path() {
        let profiles = test_profiles_both();
        let available = available_providers(&profiles);
        // All four providers default to enabled=true
        assert_eq!(available.len(), 4);
        assert!(available.contains(&AgentProvider::Claude));
        assert!(available.contains(&AgentProvider::Cursor));
    }

    #[test]
    fn friendly_spawn_error_maps_known_failures() {
        let raw = "ACP did not return a session id";
        let mapped = crate::command::agents::friendly_spawn_error(raw);
        assert_ne!(mapped, raw);

        let timeout = crate::command::agents::friendly_spawn_error(
            "ACP request timed out after 60s (method: session/load)",
        );
        assert!(!timeout.contains("method: session/load"));

        let exited = crate::command::agents::friendly_spawn_error(
            "claude process exited immediately after spawn: bad flag",
        );
        assert!(exited.contains("bad flag"));
    }

    #[test]
    fn available_providers_excludes_unconfigured() {
        let profiles = AgentProfiles {
            claude: AgentProviderConfig::default(), // enabled=true
            cursor: AgentProviderConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let available = available_providers(&profiles);
        // claude is enabled (default), cursor is disabled, pi & opencode enabled by default → 3 total.
        assert_eq!(available.len(), 3);
        assert!(available.contains(&AgentProvider::Claude));
    }

    #[test]
    fn available_providers_returns_empty_when_none_configured() {
        let profiles = AgentProfiles {
            claude: AgentProviderConfig {
                enabled: false,
                ..Default::default()
            },
            cursor: AgentProviderConfig {
                enabled: false,
                ..Default::default()
            },
            pi: AgentProviderConfig {
                enabled: false,
                ..Default::default()
            },
            opencode: AgentProviderConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        };
        let available = available_providers(&profiles);
        assert!(available.is_empty());
    }
}
