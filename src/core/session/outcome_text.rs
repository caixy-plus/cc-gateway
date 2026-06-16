//! Plain-text rendering of [`ChatCommandOutcome`] for transports without cards/keyboards (WebUI chat, etc.).

use crate::command::agents;
use crate::command::models;
use crate::config::model::{AgentProfiles, AgentProvider};
use crate::session::channel_model::AgentSession;
use crate::{t, t_fmt};

pub fn format_list_dir(dir: &str, dirs: &[(String, String)]) -> String {
    if dirs.is_empty() {
        return t!("builtin.no_subdirs").to_string();
    }
    let labeled: Vec<(String, bool)> = dirs.iter().map(|(n, _)| (n.clone(), true)).collect();
    crate::command::builtin::format_directory_list(dir, &labeled)
}

pub fn format_history(sessions: &[AgentSession]) -> String {
    if sessions.is_empty() {
        return t!("builtin.no_sessions").to_string();
    }
    let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let mut lines = vec![t!("telegram.session_history_subtitle").to_string()];
    for (idx, session) in sessions.iter().enumerate() {
        let status_dot = if session.active {
            "\u{1F7E2}"
        } else {
            "\u{26AA}"
        };
        let time = session
            .created_at
            .with_timezone(&china_tz)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        lines.push(String::new());
        lines.push(format!("{}. {} {}", idx + 1, status_dot, session.title));
        lines.push(format!("\u{1F916} {}", session.provider));
        lines.push(format!("\u{1F4C1} {}", session.work_dir));
        lines.push(format!("\u{1F552} {}", time));
        lines.push(format!("\u{1F511} {}", session.display_session_id()));
    }
    lines.join("\n")
}

pub fn format_select_agent(profiles: &AgentProfiles, current: &AgentProvider) -> String {
    agents::format_provider_picker_message(profiles, current)
}

pub fn format_select_model(
    provider: &AgentProvider,
    current: Option<&str>,
    options: &[String],
) -> String {
    let mut lines = vec![t_fmt!(
        "models.title",
        NAME = agents::provider_display_name(provider)
    )];
    lines.push(models::current_model_line(current));
    for (i, id) in options.iter().enumerate() {
        lines.push(format!("  {}. {}", i + 1, id));
    }
    lines.push(models::switch_hint_for_provider(provider));
    lines.join("\n")
}
