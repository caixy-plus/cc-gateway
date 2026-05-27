use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime::controller::AgentController;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::{t, t_fmt};

/// Paused flag so the TUI background event reader yields while
/// `interactive_select` is running, preventing key-stealing races.
pub static TUI_EVENT_READER_PAUSED: AtomicBool = AtomicBool::new(false);

/// Result of an interactive selection session.
#[derive(Debug, Clone)]
pub(crate) enum SelectAction {
    /// User pressed Enter on the item at this index.
    Selected(usize),
    /// User pressed 'x' to delete the item at this index.
    Deleted(usize),
    /// User pressed Esc or 'q' to cancel.
    Cancelled,
}

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

    pub async fn agent_history(&self, arg: &str) -> String {
        let mut sorted = load_tui_db_sessions();

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
            let ctrl = self.controller.lock().await;
            ctrl.init_work_dir(target.project).await;
            ctrl.set_pending_resume_record_id(target.cc_gateway_id.clone())
                .await;
            ctrl.set_pending_resume_provider(Some(resume_provider))
                .await;
            ctrl.set_pending_resume_session_id(
                target
                    .resume_session_id
                    .clone()
                    .or_else(|| Some(String::new())),
            )
            .await;
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

        let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();

        loop {
            let items: Vec<(String, bool)> = sorted
                .iter()
                .enumerate()
                .map(|(i, info)| {
                    let short_sid = &info.session_id[..info.session_id.len().min(8)];
                    let dt =
                        chrono::DateTime::<chrono::Utc>::from_timestamp(info.last_timestamp, 0)
                            .map(|d| {
                                d.with_timezone(&china_tz)
                                    .format("%Y-%m-%d %H:%M")
                                    .to_string()
                            })
                            .unwrap_or_else(|| "unknown".to_string());
                    let label = format!(
                        "{}. [{}] {}... (project: {}, {} messages, last: {})",
                        i + 1,
                        info.stored_provider(),
                        short_sid,
                        info.project,
                        info.message_count,
                        dt
                    );
                    (label, false)
                })
                .collect();

            if items.is_empty() {
                return t!("builtin.no_sessions").to_string();
            }

            let items_for_select = items.clone();
            let action = tokio::task::spawn_blocking(move || {
                interactive_select_for_history(&items_for_select)
            })
            .await
            .unwrap_or(SelectAction::Cancelled);

            match action {
                SelectAction::Selected(idx) => {
                    let target = sorted[idx].clone();
                    let target_sid = target.session_id.clone();
                    let resume_provider = target.stored_provider();
                    let ctrl = self.controller.lock().await;
                    ctrl.init_work_dir(target.project).await;
                    ctrl.set_pending_resume_record_id(target.cc_gateway_id.clone())
                        .await;
                    ctrl.set_pending_resume_provider(Some(resume_provider))
                        .await;
                    ctrl.set_pending_resume_session_id(
                        target
                            .resume_session_id
                            .clone()
                            .or_else(|| Some(String::new())),
                    )
                    .await;
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
                SelectAction::Deleted(idx) => {
                    if let Some(ref cc_id) = sorted[idx].cc_gateway_id {
                        // Delete from DB and memory
                        if GLOBAL_CHANNEL_SESSIONS.remove_agent_session(cc_id) {
                            // Delete history file only after the session record is deleted.
                            let file_id = sorted[idx].session_id.clone();
                            if let Some(home) = dirs::home_dir() {
                                let history_file = home
                                    .join(".cc-gateway")
                                    .join("history")
                                    .join(format!("{}.jsonl", file_id));
                                let _ = std::fs::remove_file(&history_file);
                            }
                        } else {
                            return t!("builtin.cannot_delete_active").to_string();
                        }
                    }
                    sorted.remove(idx);
                }
                SelectAction::Cancelled => {
                    return t!("builtin.selection_cancelled").to_string();
                }
            }
        }
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
    // Sort: directories first, then files
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

/// Build the rendered cell for a directory item, with optional selection highlight.
fn file_cell(name: &str, selected: bool) -> String {
    if selected {
        format!(">{}/", name)
    } else {
        format!("{}/", name)
    }
}

/// Pure function: render a file list into screen lines (single-column).
///
/// * `items`        – (name, is_dir) tuples
/// * `term_height`  – terminal rows (used to cap visible rows)
/// * `selected`     – index of the highlighted item
///
/// Returns `(lines, scroll_row)` where `lines` are the rendered strings
/// and `scroll_row` is the first visible row index.
pub fn render_file_list(
    items: &[(String, bool)],
    _term_width: u16,
    term_height: u16,
    selected: usize,
) -> (Vec<String>, usize) {
    if items.is_empty() {
        return (vec![], 0);
    }

    let header_lines = 3; // title + blank + padding
    let max_visible_rows = (term_height as usize).saturating_sub(header_lines).max(1);

    let total_rows = items.len();

    // Ensure selected item stays in view.
    let scroll_row = if selected >= max_visible_rows {
        selected.saturating_sub(max_visible_rows - 1)
    } else {
        0
    };
    let visible_rows = max_visible_rows.min(total_rows.saturating_sub(scroll_row));

    let mut lines = Vec::with_capacity(visible_rows);
    for idx in scroll_row..scroll_row + visible_rows {
        let (name, _is_dir) = &items[idx];
        lines.push(file_cell(name, idx == selected));
    }

    (lines, scroll_row)
}

/// Backend abstraction so `interactive_select` can be unit-tested without a real terminal.
pub(crate) trait SelectBackend {
    fn size(&self) -> (u16, u16);
    fn draw(&mut self, lines: &[String]);
    fn read_key(&mut self) -> Option<crossterm::event::KeyCode>;
}

pub(crate) struct RealBackend {
    prompt: String,
}

impl RealBackend {
    fn new(prompt: &str) -> Self {
        Self {
            prompt: prompt.to_string(),
        }
    }
}

impl SelectBackend for RealBackend {
    fn size(&self) -> (u16, u16) {
        crossterm::terminal::size().unwrap_or((80, 24))
    }

    fn draw(&mut self, lines: &[String]) {
        use crossterm::{cursor, terminal, QueueableCommand};
        use std::io::{stdout, Write};
        let mut stdout = stdout();
        let _ = stdout.queue(terminal::Clear(terminal::ClearType::All));
        let _ = stdout.queue(cursor::MoveTo(0, 0));
        // Use \r\n because raw mode disables automatic \n -> \r\n translation
        let _ = write!(stdout, "{}\r\n\r\n", &self.prompt);
        for line in lines {
            let _ = write!(stdout, "{}\r\n", line);
        }
        let _ = stdout.flush();
    }

    fn read_key(&mut self) -> Option<crossterm::event::KeyCode> {
        use crossterm::event::{self, Event, KeyEventKind};
        loop {
            match event::read() {
                Ok(Event::Key(key))
                    if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) =>
                {
                    return Some(key.code);
                }
                Ok(Event::Key(_)) | Ok(_) => continue,
                Err(_) => return None,
            }
        }
    }
}

pub(crate) fn interactive_select_with_backend(
    items: &[(String, bool)],
    backend: &mut dyn SelectBackend,
) -> SelectAction {
    let mut selected = 0usize;

    loop {
        let (term_width, term_height) = backend.size();
        let (lines, _scroll_row) = render_file_list(items, term_width, term_height, selected);
        backend.draw(&lines);

        match backend.read_key() {
            Some(crossterm::event::KeyCode::Up) => {
                if selected > 0 {
                    selected -= 1;
                }
            }
            Some(crossterm::event::KeyCode::Down) => {
                if selected < items.len().saturating_sub(1) {
                    selected += 1;
                }
            }
            Some(crossterm::event::KeyCode::PageUp) => {
                selected = selected.saturating_sub(lines.len());
            }
            Some(crossterm::event::KeyCode::PageDown) => {
                selected = (selected + lines.len()).min(items.len().saturating_sub(1));
            }
            Some(crossterm::event::KeyCode::Enter) => {
                return SelectAction::Selected(selected);
            }
            Some(crossterm::event::KeyCode::Char('x')) => {
                return SelectAction::Deleted(selected);
            }
            Some(crossterm::event::KeyCode::Char('q')) | Some(crossterm::event::KeyCode::Esc) => {
                return SelectAction::Cancelled;
            }
            None => return SelectAction::Cancelled,
            _ => {}
        }
    }
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

/// Load TUI-created Claude sessions from the cc-gateway database.
fn load_tui_db_sessions() -> Vec<HistorySessionInfo> {
    let channels = crate::db::load_all_channel_sessions();
    let mut result = Vec::new();

    for channel in &channels {
        if channel.platform != "tui" {
            continue;
        }
        let sessions = crate::db::load_agent_sessions_by_channel_id(&channel.id);
        for s in sessions {
            let resume_session_id = s.provider_session_id.clone();
            result.push(HistorySessionInfo {
                session_id: resume_session_id.clone().unwrap_or_else(|| s.id.clone()),
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

pub(crate) fn interactive_select(items: &[(String, bool)]) -> SelectAction {
    interactive_select_with_prompt(items, crate::t!("builtin.select_dir_prompt"))
}

pub(crate) fn interactive_select_for_history(items: &[(String, bool)]) -> SelectAction {
    interactive_select_with_prompt(items, crate::t!("builtin.select_history_prompt"))
}

pub(crate) fn interactive_select_with_prompt(items: &[(String, bool)], prompt: &str) -> SelectAction {
    struct PauseGuard;
    impl Drop for PauseGuard {
        fn drop(&mut self) {
            TUI_EVENT_READER_PAUSED.store(false, Ordering::Relaxed);
        }
    }

    TUI_EVENT_READER_PAUSED.store(true, Ordering::Relaxed);
    let _pause_guard = PauseGuard;
    // Give the TUI background event reader time to see the flag and stop polling.
    std::thread::sleep(std::time::Duration::from_millis(30));

    let raw_was_enabled = crossterm::terminal::is_raw_mode_enabled().unwrap_or(false);
    if !raw_was_enabled && crossterm::terminal::enable_raw_mode().is_err() {
        return SelectAction::Cancelled;
    }

    let mut backend = RealBackend::new(prompt);
    let result = interactive_select_with_backend(items, &mut backend);
    if !raw_was_enabled {
        let _ = crossterm::terminal::disable_raw_mode();
    }
    result
}
