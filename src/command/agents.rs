use crate::command::builtin::{interactive_select_with_prompt, SelectAction};
use crate::config::model::AgentProvider;
use crate::{t, t_fmt};

/// Providers whose `cli_path` has been explicitly configured.
/// Only these appear in the `/agents` picker.
pub fn available_providers(profiles: &crate::config::model::AgentProfiles) -> Vec<AgentProvider> {
    let mut providers = Vec::new();
    if profiles.claude.cli_path.is_some() {
        providers.push(AgentProvider::Claude);
    }
    if profiles.cursor.cli_path.is_some() {
        providers.push(AgentProvider::Cursor);
    }
    if profiles.pi.cli_path.is_some() {
        providers.push(AgentProvider::Pi);
    }
    providers
}

pub fn provider_display_name(provider: &AgentProvider) -> &'static str {
    match provider {
        AgentProvider::Claude => "claude",
        AgentProvider::Cursor => "cursor",
        AgentProvider::Pi => "pi",
    }
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

pub fn interactive_select_provider(
    profiles: &crate::config::model::AgentProfiles,
    current_default: &AgentProvider,
) -> SelectAction {
    let items = build_provider_items(profiles, current_default);
    interactive_select_with_prompt(&items, t!("builtin.select_agent_prompt"))
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

pub fn session_stopped_message(provider: &AgentProvider) -> String {
    t_fmt!(
        "builtin.session_stopped",
        NAME = provider_display_name(provider)
    )
}

pub fn failed_start_agent_message(provider: &AgentProvider, err: impl std::fmt::Display) -> String {
    t_fmt!(
        "builtin.failed_start_agent",
        NAME = provider_display_name(provider),
        ERR = err
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::builtin::SelectBackend;
    use crate::config::model::{AgentProfiles, AgentProviderConfig};

    fn test_profiles_both() -> AgentProfiles {
        AgentProfiles {
            claude: AgentProviderConfig {
                cli_path: Some("claude".to_string()),
                ..Default::default()
            },
            cursor: AgentProviderConfig {
                cli_path: Some("agent".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    struct TestBackend {
        keys: std::collections::VecDeque<crossterm::event::KeyCode>,
        frames: Vec<Vec<String>>,
    }

    impl SelectBackend for TestBackend {
        fn size(&self) -> (u16, u16) {
            (80, 24)
        }

        fn draw(&mut self, lines: &[String]) {
            self.frames.push(lines.to_vec());
        }

        fn read_key(&mut self) -> Option<crossterm::event::KeyCode> {
            self.keys.pop_front()
        }
    }

    #[test]
    fn build_provider_items_marks_current_default() {
        let profiles = test_profiles_both();
        let items = build_provider_items(&profiles, &AgentProvider::Cursor);
        assert_eq!(items.len(), 2);
        assert!(items[0].0.contains("claude"));
        assert!(items[1].0.contains('*') || items[1].0.contains("cursor"));
    }

    #[test]
    fn session_started_message_uses_provider_display_name() {
        let msg = session_started_message(&AgentProvider::Cursor, "/tmp/work");
        assert!(msg.contains("cursor"));
        assert!(!msg.contains("claude"));
    }

    #[test]
    fn interactive_select_provider_picks_index() {
        let profiles = test_profiles_both();
        let items = build_provider_items(&profiles, &AgentProvider::Claude);
        let mut backend = TestBackend {
            keys: std::collections::VecDeque::from([
                crossterm::event::KeyCode::Down,
                crossterm::event::KeyCode::Enter,
            ]),
            frames: Vec::new(),
        };
        let action = crate::command::builtin::interactive_select_with_backend(&items, &mut backend);
        assert!(matches!(action, SelectAction::Selected(1)));
        assert_eq!(provider_at_index(&profiles, 1), Some(AgentProvider::Cursor));
    }

    #[test]
    fn available_providers_filters_by_configured_cli_path() {
        let profiles = test_profiles_both();
        let available = available_providers(&profiles);
        assert_eq!(available.len(), 2);
        assert!(available.contains(&AgentProvider::Claude));
        assert!(available.contains(&AgentProvider::Cursor));
    }

    #[test]
    fn available_providers_excludes_unconfigured() {
        let profiles = AgentProfiles {
            claude: AgentProviderConfig {
                cli_path: Some("claude".to_string()),
                ..Default::default()
            },
            // cursor has no cli_path set
            ..Default::default()
        };
        let available = available_providers(&profiles);
        assert_eq!(available.len(), 1);
        assert_eq!(available[0], AgentProvider::Claude);
    }

    #[test]
    fn available_providers_returns_empty_when_none_configured() {
        let profiles = AgentProfiles::default();
        let available = available_providers(&profiles);
        assert!(available.is_empty());
    }
}
