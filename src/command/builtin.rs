use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::{t, t_fmt};
pub struct BuiltinCommands {
    controller: Arc<Mutex<ClaudeController>>,
    default_dir: String,
}

impl BuiltinCommands {
    pub fn new(controller: Arc<Mutex<ClaudeController>>, default_dir: &str) -> Self {
        Self {
            controller,
            default_dir: default_dir.to_string(),
        }
    }

    pub async fn handle(&self, message: &str) -> Option<String> {
        let parts: Vec<&str> = message.splitn(2, ' ').collect();
        let cmd = parts[0];
        let arg = parts.get(1).map(|s| *s).unwrap_or("");

        match cmd {
            "/help" => Some(self.help()),
            "/quit" => Some(self.quit().await),
            "/cd" => Some(self.cd(arg).await),
            "/cd_default" => Some(self.cd_default().await),
            "/claude" => Some(self.claude(arg).await),
            "/pwd" => Some(self.pwd().await),
            "/ll" => Some(self.ll().await),
            _ => None,
        }
    }

    fn help(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n\n{}",
            t!("builtin.help_title"),
            t!("builtin.help_help"),
            t!("builtin.help_quit"),
            t!("builtin.help_cd"),
            t!("builtin.help_cd_default"),
            t!("builtin.help_claude"),
            t!("builtin.help_pwd"),
            t!("builtin.help_ll"),
            t!("builtin.help_any_text")
        )
    }

    async fn quit(&self) -> String {
        let ctrl = self.controller.lock().await;
        match ctrl.stop_session().await {
            Ok(()) => t!("builtin.session_stopped").to_string(),
            Err(e) => t_fmt!("builtin.failed_stop_session", ERR = e),
        }
    }

    async fn cd(&self, path: &str) -> String {
        if path.is_empty() {
            return t!("builtin.cd_usage").to_string();
        }

        let ctrl = self.controller.lock().await;
        let current_dir = ctrl.get_work_dir().await;
        let base = if current_dir.is_empty() {
            shellexpand::tilde(&self.default_dir).to_string()
        } else {
            current_dir
        };

        let expanded = shellexpand::tilde(path).to_string();
        let target = std::path::Path::new(&base).join(&expanded);
        let target_str = target.to_string_lossy().to_string();

        let canonical = std::path::PathBuf::from(&target_str);
        let canonical = canonical.canonicalize().unwrap_or(canonical);

        if !canonical.is_dir() {
            return t_fmt!("builtin.invalid_path", PATH = canonical.display());
        }

        let path_str = canonical.to_string_lossy().to_string();
        if let Err(e) = crate::claude::controller::ensure_under_home(&path_str) {
            return e.to_string();
        }

        ctrl.init_work_dir(path_str.clone()).await;
        t_fmt!("builtin.dir_changed", PATH = path_str)
    }

    async fn cd_default(&self) -> String {
        let dir = shellexpand::tilde(&self.default_dir).to_string();
        self.cd(&dir).await
    }

    async fn claude(&self, args: &str) -> String {
        let ctrl = self.controller.lock().await;
        let work_dir = ctrl.get_work_dir().await;
        let dir = if work_dir.is_empty() {
            shellexpand::tilde(&self.default_dir).to_string()
        } else {
            work_dir
        };

        let extra_args: Vec<String> = if args.is_empty() {
            vec![]
        } else {
            args.split_whitespace().map(|s| s.to_string()).collect()
        };

        match ctrl.start_session(dir.clone(), extra_args).await {
            Ok(()) => t_fmt!("builtin.session_started", DIR = dir),
            Err(e) => t_fmt!("builtin.failed_start_claude", ERR = e),
        }
    }

    async fn pwd(&self) -> String {
        let ctrl = self.controller.lock().await;
        let work_dir = ctrl.get_work_dir().await;
        let dir = if work_dir.is_empty() {
            shellexpand::tilde(&self.default_dir).to_string()
        } else {
            work_dir
        };
        t_fmt!("builtin.current_dir", DIR = dir)
    }

    async fn ll(&self) -> String {
        let ctrl = self.controller.lock().await;
        let work_dir = ctrl.get_work_dir().await;
        let dir = if work_dir.is_empty() {
            shellexpand::tilde(&self.default_dir).to_string()
        } else {
            work_dir
        };
        drop(ctrl);

        // Ensure the directory is under the user's home directory
        if let Err(e) = crate::claude::controller::ensure_under_home(&dir) {
            return t_fmt!("builtin.access_denied", ERR = e);
        }

        let items = match list_directory_items(&dir) {
            Ok(items) => items,
            Err(e) => return t_fmt!("builtin.failed_list_dir", ERR = e),
        };

        // Only show directories
        let dirs: Vec<(String, bool)> = items.into_iter().filter(|(_, is_dir)| *is_dir).collect();

        if dirs.is_empty() {
            return t!("builtin.no_subdirs").to_string();
        }

        let selected = tokio::task::spawn_blocking(move || interactive_select(&dirs))
            .await
            .unwrap_or(None);

        match selected {
            Some((name, _is_dir)) => {
                let path = std::path::Path::new(&dir).join(&name);
                let path_str = path.to_string_lossy().to_string();
                let ctrl = self.controller.lock().await;
                ctrl.init_work_dir(path_str.clone()).await;
                t_fmt!("builtin.changed_dir", PATH = path_str)
            }
            None => t!("builtin.selection_cancelled").to_string(),
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
    items.sort_by(|a, b| {
        match (a.1, b.1) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.cmp(&b.0),
        }
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
trait SelectBackend {
    fn size(&self) -> (u16, u16);
    fn draw(&mut self, lines: &[String]);
    fn read_key(&mut self) -> Option<crossterm::event::KeyCode>;
}

struct RealBackend;

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
        let _ = write!(stdout, "{}\r\n\r\n", t!("builtin.select_dir_prompt"));
        for line in lines {
            let _ = write!(stdout, "{}\r\n", line);
        }
        let _ = stdout.flush();
    }

    fn read_key(&mut self) -> Option<crossterm::event::KeyCode> {
        use crossterm::event::{self, Event};
        if let Ok(Event::Key(key)) = event::read() {
            Some(key.code)
        } else {
            None
        }
    }
}

#[cfg(test)]
#[derive(Debug)]
struct TestBackend {
    term_size: (u16, u16),
    keys: Vec<crossterm::event::KeyCode>,
    key_idx: usize,
    pub frames: Vec<Vec<String>>,
}

#[cfg(test)]
impl TestBackend {
    fn new(term_size: (u16, u16), keys: Vec<crossterm::event::KeyCode>) -> Self {
        Self {
            term_size,
            keys,
            key_idx: 0,
            frames: Vec::new(),
        }
    }
}

#[cfg(test)]
impl SelectBackend for TestBackend {
    fn size(&self) -> (u16, u16) {
        self.term_size
    }

    fn draw(&mut self, lines: &[String]) {
        self.frames.push(lines.to_vec());
    }

    fn read_key(&mut self) -> Option<crossterm::event::KeyCode> {
        if self.key_idx < self.keys.len() {
            let k = self.keys[self.key_idx];
            self.key_idx += 1;
            Some(k)
        } else {
            None
        }
    }
}

fn interactive_select_with_backend(
    items: &[(String, bool)],
    backend: &mut dyn SelectBackend,
) -> Option<(String, bool)> {
    let mut selected = 0usize;

    loop {
        let (term_width, term_height) = backend.size();
        let (lines, _scroll_row) = render_file_list(items, term_width, term_height, selected);
        backend.draw(&lines);

        match backend.read_key() {
            Some(crossterm::event::KeyCode::Up) => {
                if selected > 0 { selected -= 1; }
            }
            Some(crossterm::event::KeyCode::Down) => {
                if selected < items.len().saturating_sub(1) { selected += 1; }
            }
            Some(crossterm::event::KeyCode::PageUp) => {
                selected = selected.saturating_sub(lines.len());
            }
            Some(crossterm::event::KeyCode::PageDown) => {
                selected = (selected + lines.len()).min(items.len().saturating_sub(1));
            }
            Some(crossterm::event::KeyCode::Enter) => {
                return Some(items[selected].clone());
            }
            Some(crossterm::event::KeyCode::Char('q'))
            | Some(crossterm::event::KeyCode::Esc) => {
                return None;
            }
            _ => {}
        }
    }
}

fn interactive_select(items: &[(String, bool)]) -> Option<(String, bool)> {
    use std::io::{self, IsTerminal};

    if !io::stdin().is_terminal() {
        return None;
    }

    let _ = crossterm::terminal::enable_raw_mode();
    let mut backend = RealBackend;
    let result = interactive_select_with_backend(items, &mut backend);
    let _ = crossterm::terminal::disable_raw_mode();
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::ClaudeConfig;

    fn setup() -> BuiltinCommands {
        let config = ClaudeConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(config)));
        BuiltinCommands::new(controller, "~")
    }

    #[tokio::test]
    async fn test_help_returns_non_empty_with_expected_commands() {
        let builtin = setup();
        let response = builtin.handle("/help").await.unwrap();
        assert!(!response.is_empty());
        assert!(response.contains("/help"));
        assert!(response.contains("/pwd"));
        assert!(response.contains("/cd"));
        assert!(response.contains("/claude"));
        assert!(response.contains("/ll"));
        assert!(response.contains("/quit"));
    }

    #[tokio::test]
    async fn test_pwd_contains_current_directory() {
        let builtin = setup();
        let response = builtin.handle("/pwd").await.unwrap();
        assert!(response.contains("Current directory"));
    }

    #[tokio::test]
    async fn test_ll_returns_non_empty() {
        let builtin = setup();
        let response = builtin.handle("/ll").await.unwrap();
        assert!(!response.is_empty());
    }

    #[tokio::test]
    async fn test_cd_empty_arg_returns_usage() {
        let builtin = setup();
        let response = builtin.handle("/cd").await.unwrap();
        assert_eq!(response, "Usage: /cd <path>");
    }

    #[tokio::test]
    async fn test_cd_invalid_path_returns_error() {
        let builtin = setup();
        let response = builtin.handle("/cd /nonexistent_path_12345").await.unwrap();
        assert!(response.starts_with("Invalid path:"));
    }

    #[tokio::test]
    async fn test_cd_parent_directory() {
        let builtin = setup();
        // Use the project's src directory as a known existing subdirectory
        let current = std::env::current_dir().unwrap();
        let subdir = current.join("src");
        let parent = current.to_string_lossy().to_string();

        let response1 = builtin.handle(&format!("/cd {}", subdir.display())).await.unwrap();
        assert!(response1.starts_with("Working directory changed to:"));

        let response2 = builtin.handle("/cd ..").await.unwrap();
        assert!(response2.starts_with("Working directory changed to:"));
        assert!(response2.contains(&parent), "Expected response to contain parent directory {}, got: {}", parent, response2);
    }

    #[tokio::test]
    async fn test_cd_outside_home_denied() {
        let builtin = setup();
        // Try to cd to a directory outside home
        let response = builtin.handle("/cd /tmp").await.unwrap();
        assert!(
            response.starts_with("Access denied:"),
            "Expected access denied for /tmp, got: {}",
            response
        );
        // Ensure there is no duplicated "Access denied" prefix
        assert!(
            !response.contains("Access denied: Access denied:"),
            "Duplicated access denied prefix: {}",
            response
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_claude_starts_session() {
        let builtin = setup();
        let _response = builtin.handle("/claude").await.unwrap();
        // Spawns subprocess; marked ignore
    }

    #[tokio::test]
    #[ignore]
    async fn test_cd_valid_path_changes_work_dir() {
        let builtin = setup();
        let _response = builtin.handle("/cd /tmp").await.unwrap();
        // Spawns subprocess via set_work_dir; marked ignore
    }

    // ------------------------------------------------------------------
    // render_file_list unit tests – these verify layout maths without
    // touching the real terminal.
    // ------------------------------------------------------------------

    #[test]
    fn test_render_single_column() {
        let items = vec![
            (".git".to_string(), true),
            ("assets".to_string(), true),
            ("work".to_string(), true),
        ];
        let (lines, scroll) = render_file_list(&items, 20, 10, 0);
        assert_eq!(scroll, 0);
        assert_eq!(lines.len(), 3);
        // First item is selected (index 0)
        assert_eq!(lines[0], ">.git/");
        assert_eq!(lines[1], "assets/");
        assert_eq!(lines[2], "work/");
    }

    #[test]
    fn test_render_single_column_selected() {
        let items = vec![
            ("a".to_string(), true),
            ("b".to_string(), true),
            ("c".to_string(), true),
        ];
        let (lines, _scroll) = render_file_list(&items, 80, 10, 1);
        assert_eq!(lines[0], "a/");
        assert_eq!(lines[1], ">b/");
        assert_eq!(lines[2], "c/");
    }

    #[test]
    fn test_render_lines_left_aligned() {
        let items = vec![
            ("short".to_string(), true),
            ("very_long_name".to_string(), true),
            ("mid".to_string(), true),
        ];
        let (lines, _scroll) = render_file_list(&items, 80, 10, 0);
        for line in &lines {
            assert!(
                !line.starts_with(' '),
                "rows must start flush-left, got: {:?}",
                line
            );
        }
    }

    #[test]
    fn test_render_no_ansi_codes() {
        let items = vec![
            ("alpha".to_string(), true),
            ("beta".to_string(), true),
        ];
        let (lines, _scroll) = render_file_list(&items, 80, 10, 0);
        for line in &lines {
            assert!(
                !line.contains('\x1b'),
                "no ANSI escape codes expected, got: {:?}",
                line
            );
        }
    }

    #[test]
    fn test_render_scrolls_when_selected_out_of_view() {
        let items: Vec<(String, bool)> = (0..20)
            .map(|i| (format!("item{:02}", i), true))
            .collect();
        // Tiny terminal – only 5 visible data rows
        let (lines, scroll) = render_file_list(&items, 40, 8, 15);
        assert!(
            scroll > 0,
            "scroll_row should move down when selected is near bottom"
        );
        // The selected item (item15) must be visible in the output
        let found = lines.iter().any(|l| l.contains("item15"));
        assert!(found, "selected item must be in visible lines: {:?}", lines);
    }

    #[test]
    fn test_interactive_select_down_enter() {
        use crossterm::event::KeyCode;
        let items = vec![
            (".git".to_string(), true),
            ("assets".to_string(), true),
            ("src".to_string(), true),
        ];
        let keys = vec![KeyCode::Down, KeyCode::Enter];
        let mut backend = TestBackend::new((40, 10), keys);
        let result = interactive_select_with_backend(&items, &mut backend);

        assert_eq!(result, Some(("assets".to_string(), true)));
        assert!(!backend.frames.is_empty(), "should have rendered at least one frame");

        // First frame: item 0 selected
        let frame0 = &backend.frames[0];
        assert!(frame0.iter().any(|l| l == ">.git/"));

        // Second frame: after Down, item 1 selected
        let frame1 = &backend.frames[1];
        assert!(frame1.iter().any(|l| l == ">assets/"));
    }

    #[test]
    fn test_interactive_select_cancel_with_q() {
        use crossterm::event::KeyCode;
        let items = vec![
            ("a".to_string(), true),
            ("b".to_string(), true),
        ];
        let keys = vec![KeyCode::Char('q')];
        let mut backend = TestBackend::new((40, 10), keys);
        let result = interactive_select_with_backend(&items, &mut backend);
        assert_eq!(result, None);
        assert_eq!(backend.frames.len(), 1);
    }

    #[test]
    fn test_interactive_select_single_column_frames() {
        use crossterm::event::KeyCode;
        let items = vec![
            ("alpha".to_string(), true),
            ("beta".to_string(), true),
            ("gamma".to_string(), true),
            ("delta".to_string(), true),
        ];
        let keys = vec![KeyCode::Down, KeyCode::Down, KeyCode::Enter];
        let mut backend = TestBackend::new((44, 10), keys);
        let result = interactive_select_with_backend(&items, &mut backend);

        assert_eq!(result, Some(("gamma".to_string(), true)));

        // Verify single-column layout in the first frame
        let frame0 = &backend.frames[0];
        assert_eq!(frame0[0], ">alpha/");
        assert_eq!(frame0[1], "beta/");
        assert_eq!(frame0[2], "gamma/");
        assert_eq!(frame0[3], "delta/");
    }
}
