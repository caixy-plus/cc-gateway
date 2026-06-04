use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime::controller::AgentController;
use crate::{t, t_fmt};

pub struct BuiltinCommands {
    controller: Arc<Mutex<AgentController>>,
}

impl BuiltinCommands {
    pub fn new(controller: Arc<Mutex<AgentController>>, _default_dir: &str) -> Self {
        Self { controller }
    }

    pub fn help_text(&self) -> String {
        t!("builtin.help").to_string()
    }

    pub fn session_help_text(&self) -> String {
        t!("builtin.session_help").to_string()
    }

    pub async fn agent_history(&self, arg: &str) -> String {
        let sorted = load_agent_history_sessions();

        if sorted.is_empty() {
            return t!("builtin.no_sessions").to_string();
        }

        if let Ok(idx) = arg.parse::<usize>() {
            if idx == 0 || idx > sorted.len() {
                return t!("builtin.invalid_history_index").to_string();
            }
            let target = sorted[idx - 1].clone();
            let target_sid = target.session_id.clone();
            let resume_provider = target.stored_provider();
            if !crate::command::agents::provider_supports_session_resume(&resume_provider) {
                return t!("builtin.pi_resume_not_supported").to_string();
            }
            let resume_sid = resume_session_id_for_history(&target);
            let ctrl = self.controller.lock().await;
            ctrl.init_work_dir(target.project).await;
            ctrl.set_pending_resume_record_id(target.cc_gateway_id.clone())
                .await;
            ctrl.set_pending_resume_provider(Some(resume_provider.clone()))
                .await;
            ctrl.set_pending_resume_session_id(resume_sid).await;
            if target.resume_session_id.is_none() {
                return t_fmt!(
                    "builtin.resume_session_missing_id",
                    SID = &target_sid[..target_sid.len().min(8)]
                );
            }
            return t_fmt!(
                "builtin.resume_session_set",
                SID = &target_sid[..target_sid.len().min(8)]
            );
        }

        format_agent_history_list(&sorted)
    }
}

pub fn list_directory_items(dir: &str) -> Result<Vec<(String, bool)>, std::io::Error> {
    let mut items = Vec::new();
    let expanded = shellexpand::tilde(dir).to_string();
    let path = std::path::Path::new(&expanded);
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type()?.is_dir();
        items.push((name, is_dir));
    }
    items.sort_by(|a, b| match (a.1, b.1) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.0.cmp(&b.0),
    });
    Ok(items)
}

/// List directories under `dir`, returning (name, full_path) pairs sorted by name.
pub fn list_directory_paths(dir: &str) -> Result<Vec<(String, String)>, std::io::Error> {
    let mut items = Vec::new();
    let expanded = shellexpand::tilde(dir).to_string();
    let path = std::path::Path::new(&expanded);
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let full = entry.path().to_string_lossy().to_string();
            items.push((name, full));
        }
    }
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(items)
}

/// Plain-text `/ll` listing for router.execute (WebUI message API, etc.).
pub fn format_directory_list(target: &str, dirs: &[(String, bool)]) -> String {
    if dirs.is_empty() {
        return t!("builtin.no_subdirs").to_string();
    }
    let mut out = t_fmt!("builtin.dir_list_header", DIR = target);
    for (name, _) in dirs {
        out.push_str(&format!("\n  {name}/"));
    }
    out.push_str(&format!("\n{}", t!("builtin.dir_list_hint")));
    out
}

#[derive(Clone, Debug)]
pub(crate) struct HistorySessionInfo {
    pub session_id: String,
    pub resume_session_id: Option<String>,
    pub cc_gateway_id: Option<String>,
    pub provider: String,
    pub project: String,
    pub last_timestamp: i64,
    pub message_count: usize,
}

impl HistorySessionInfo {
    pub(crate) fn stored_provider(&self) -> crate::config::model::AgentProvider {
        crate::config::model::AgentProvider::parse_str(&self.provider)
    }
}

fn resume_session_id_for_history(target: &HistorySessionInfo) -> Option<String> {
    target.resume_session_id.clone()
}

fn load_agent_history_sessions() -> Vec<HistorySessionInfo> {
    let channels = crate::db::load_all_channel_sessions();
    let mut result = Vec::new();

    for channel in &channels {
        let sessions = crate::db::load_agent_sessions_by_channel_id(&channel.id);
        for s in sessions {
            let resume_session_id = s.provider_session_id.clone();
            result.push(HistorySessionInfo {
                session_id: s.display_session_id().to_string(),
                resume_session_id,
                cc_gateway_id: Some(s.id),
                provider: s.provider.clone(),
                project: s.work_dir,
                last_timestamp: s.updated_at.unwrap_or(s.created_at).timestamp(),
                message_count: 0,
            });
        }
    }

    result.sort_by(|a, b| b.last_timestamp.cmp(&a.last_timestamp));
    result
}

fn format_agent_history_list(sorted: &[HistorySessionInfo]) -> String {
    let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
    let mut lines = Vec::new();
    for (i, info) in sorted.iter().enumerate() {
        let short_sid = &info.session_id[..info.session_id.len().min(8)];
        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(info.last_timestamp, 0)
            .map(|d| {
                d.with_timezone(&china_tz)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| "unknown".to_string());
        lines.push(format!(
            "{}. [{}] {}... (project: {}, {} messages, last: {})",
            i + 1,
            info.stored_provider(),
            short_sid,
            info.project,
            info.message_count,
            dt
        ));
    }
    lines.push(String::new());
    lines.push(t!("builtin.agent_history_hint").to_string());
    lines.join("\n")
}
