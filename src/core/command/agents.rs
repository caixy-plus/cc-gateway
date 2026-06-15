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
    crate::config::agent_registry::capabilities_for(provider).session_resume
}

/// Whether the provider supports manual context compaction (`/compact`).
pub fn provider_supports_context_compact(provider: &AgentProvider) -> bool {
    crate::config::agent_registry::capabilities_for(provider).context_compact
}

/// Whether `/compact` is sent as a user-visible `/compact` message (Claude stream-json).
pub fn provider_compact_via_user_message(provider: &AgentProvider) -> bool {
    crate::config::agent_registry::capabilities_for(provider).compact_via_user_message
}

/// Whether the provider supports initializing project memory files (`/init`, e.g. Claude `CLAUDE.md`).
pub fn provider_supports_memory_init(provider: &AgentProvider) -> bool {
    crate::config::agent_registry::capabilities_for(provider).memory_init
}

/// User-visible message after a successful `/compact`.
pub fn compact_success_message(summary: &str) -> String {
    let summary = summary.trim();
    if summary.is_empty() {
        return t!("builtin.compact_ok").to_string();
    }
    const MAX_CHARS: usize = 240;
    let len = summary.chars().count();
    let display = if len > MAX_CHARS {
        let prefix: String = summary.chars().take(MAX_CHARS).collect();
        format!("{prefix}...")
    } else {
        summary.to_string()
    };
    t_fmt!("builtin.compact_ok_with_summary", SUMMARY = display)
}

/// Message shown when a session is restarted (e.g. user clicks "Restart Session" or `/agent-history N`).
pub fn session_restarted_message(provider: &AgentProvider, dir: &str) -> String {
    if crate::config::agent_registry::capabilities_for(provider).restart_shows_fresh_hint {
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
/// Auth/subscription guidance is generic; use [`friendly_spawn_error_for`] when the
/// provider is known so users get the right sign-in command.
pub fn friendly_spawn_error(raw: &str) -> String {
    friendly_spawn_error_for(None, raw)
}

pub fn friendly_spawn_error_for(provider: Option<&AgentProvider>, raw: &str) -> String {
    if raw.contains("ACP did not return a session id") {
        return t!("agent.acp_no_session_id").to_string();
    }
    if raw.contains("ACP request timed out") || raw.contains("等待智能体响应超时") {
        return t!("agent.acp_request_timeout").to_string();
    }
    if raw.contains("authRequired")
        || raw.contains("Authentication required")
        || (raw.contains("-32000") && (raw.contains("auth") || raw.contains("login")))
    {
        return match provider {
            Some(AgentProvider::Kimi) => t!("agent.kimi_auth_required").to_string(),
            Some(AgentProvider::Gemini) => t!("agent.gemini_auth_required").to_string(),
            _ => t!("agent.auth_required").to_string(),
        };
    }
    if raw.contains("subscription")
        || raw.contains("套餐")
        || raw.contains("订阅")
        || raw.contains("membership")
        || raw.contains("billing")
        || raw.contains("quota")
        || raw.contains("402")
    {
        return match provider {
            Some(AgentProvider::Kimi) => t!("agent.kimi_subscription_required").to_string(),
            _ => t!("agent.subscription_required").to_string(),
        };
    }
    if raw.contains("Kimi returned no output") || raw.contains("Kimi 本轮未返回") {
        return t!("agent.kimi_no_response").to_string();
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
        ERR = friendly_spawn_error_for(Some(provider), &raw)
    )
}

/// User-visible reply when `/stop` is used but the agent is already idle.
pub fn stop_already_idle_message(provider: &AgentProvider) -> String {
    if crate::config::agent_registry::capabilities_for(provider).uses_claude_idle_copy {
        t!("builtin.stop_already_idle_claude").to_string()
    } else {
        t!("builtin.stop_already_idle").to_string()
    }
}

/// User-visible reply after `/stop` succeeds.
pub fn stop_sent_message(provider: &AgentProvider) -> String {
    match provider {
        AgentProvider::Claude => t!("builtin.stop_sent_claude").to_string(),
        AgentProvider::Codex => t!("builtin.stop_sent_codex").to_string(),
        AgentProvider::Cursor => t!("builtin.stop_sent_cursor").to_string(),
        AgentProvider::Pi => t!("builtin.stop_sent_pi").to_string(),
        AgentProvider::OpenCode => t!("builtin.stop_sent_opencode").to_string(),
        AgentProvider::Kimi => t!("builtin.stop_sent_kimi").to_string(),
        AgentProvider::Gemini => t!("builtin.stop_sent_gemini").to_string(),
        AgentProvider::Qoder => t!("builtin.stop_sent_qoder").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::AgentProfiles;

    fn test_profiles_both() -> AgentProfiles {
        AgentProfiles::default()
    }

    #[test]
    fn build_provider_items_marks_current_default() {
        let profiles = test_profiles_both();
        let items = build_provider_items(&profiles, &AgentProvider::Cursor);
        // All registered providers enabled by default
        assert_eq!(
            items.len(),
            crate::config::agent_registry::AGENT_PROVIDER_DEFS.len()
        );
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
    fn context_compact_supported_only_for_claude_and_pi() {
        assert!(provider_supports_context_compact(&AgentProvider::Claude));
        assert!(provider_supports_context_compact(&AgentProvider::Pi));
        assert!(!provider_supports_context_compact(&AgentProvider::Cursor));
        assert!(!provider_supports_context_compact(&AgentProvider::OpenCode));
    }

    #[test]
    fn memory_init_supported_only_for_claude() {
        assert!(provider_supports_memory_init(&AgentProvider::Claude));
        assert!(!provider_supports_memory_init(&AgentProvider::Pi));
        assert!(!provider_supports_memory_init(&AgentProvider::Cursor));
        assert!(!provider_supports_memory_init(&AgentProvider::OpenCode));
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
        // All registered providers default to enabled=true
        assert_eq!(
            available.len(),
            crate::config::agent_registry::AGENT_PROVIDER_DEFS.len()
        );
        assert!(available.contains(&AgentProvider::Claude));
        assert!(available.contains(&AgentProvider::Cursor));
    }

    #[test]
    fn friendly_spawn_error_maps_kimi_auth_and_subscription() {
        let auth = crate::command::agents::friendly_spawn_error("authRequired: login required");
        assert_ne!(auth, "authRequired: login required");
        let sub = crate::command::agents::friendly_spawn_error("subscription inactive");
        assert_ne!(sub, "subscription inactive");
    }

    #[test]
    fn friendly_spawn_error_auth_guidance_is_provider_scoped() {
        let kimi = friendly_spawn_error_for(Some(&AgentProvider::Kimi), "authRequired");
        let gemini = friendly_spawn_error_for(Some(&AgentProvider::Gemini), "authRequired");
        let unknown = friendly_spawn_error_for(None, "authRequired");
        assert!(kimi.contains("kimi"));
        assert!(gemini.to_lowercase().contains("gemini"));
        assert!(!unknown.to_lowercase().contains("kimi"));
        assert!(!unknown.to_lowercase().contains("gemini"));
    }

    #[test]
    fn friendly_spawn_error_subscription_guidance_only_kimi_branded_for_kimi() {
        let kimi = friendly_spawn_error_for(Some(&AgentProvider::Kimi), "402 membership inactive");
        let other = friendly_spawn_error_for(Some(&AgentProvider::Claude), "quota exceeded");
        assert!(kimi.contains("Kimi") || kimi.contains("kimi"));
        assert!(!other.contains("Kimi") && !other.contains("kimi"));
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
        let mut profiles = AgentProfiles::default();
        profiles.profile_mut(&AgentProvider::Cursor).enabled = false;
        let available = available_providers(&profiles);
        // Only cursor was disabled; every other registered provider stays enabled by default.
        assert_eq!(
            available.len(),
            crate::config::agent_registry::AGENT_PROVIDER_DEFS.len() - 1
        );
        assert!(available.contains(&AgentProvider::Claude));
        assert!(!available.contains(&AgentProvider::Cursor));
    }

    #[test]
    fn available_providers_returns_empty_when_none_configured() {
        let mut profiles = AgentProfiles::default();
        for def in crate::config::agent_registry::AGENT_PROVIDER_DEFS {
            profiles.profile_mut(&def.provider).enabled = false;
        }
        let available = available_providers(&profiles);
        assert!(available.is_empty());
    }
}
