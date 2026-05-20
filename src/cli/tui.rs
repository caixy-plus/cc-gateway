use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use regex::Regex;
use std::io;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use unicode_width::UnicodeWidthStr;

use crate::command::builtin::TUI_EVENT_READER_PAUSED;

use crate::claude::controller::{ClaudeController, ControllerEvent};
use crate::command::router::CommandRouter;
use crate::cli::interactive::format_banner;
use crate::t;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Strip ANSI escape sequences so text renders cleanly in ratatui.
fn strip_ansi(s: &str) -> String {
    thread_local! {
        static RE: Regex = Regex::new("\x1b\\[[0-9;]*m").unwrap();
    }
    RE.with(|re| re.replace_all(s, "").to_string())
}

/// Split a string into display lines (respecting embedded newlines).
fn to_lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    s.lines().map(|l| l.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Message model
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq)]
enum MsgRole {
    User,
    Claude,
    System,
    Thinking,
    Tool,
    Error,
}

#[derive(Clone)]
struct ChatMessage {
    role: MsgRole,
    lines: Vec<String>,
}

impl ChatMessage {
    fn new(role: MsgRole, text: &str) -> Self {
        Self { role, lines: to_lines(&strip_ansi(text)) }
    }

    fn append(&mut self, text: &str) {
        let clean = strip_ansi(text);
        if let Some(last) = self.lines.last_mut() {
            last.push_str(&clean);
        } else {
            self.lines.push(clean);
        }
    }
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct App {
    messages: Vec<ChatMessage>,
    input: String,
    input_cursor: usize,
    scroll_offset: usize,
    claude_busy: bool,
    needs_claude_response: bool,
    session_active: bool,
    work_dir: String,
    banner_shown: bool,
    /// Track consecutive thinking events for dedup.
    last_was_thinking: bool,
    /// Available slash commands for Tab completion.
    commands: Vec<String>,
    /// Current completion match list (recomputed on input change).
    completion_matches: Vec<String>,
    /// Index within completion_matches for cycling.
    completion_index: usize,
    /// Last input that triggered a completion (to detect change).
    last_input_for_completion: String,
}

impl App {
    fn new() -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            claude_busy: false,
            needs_claude_response: false,
            session_active: false,
            work_dir: String::new(),
            banner_shown: false,
            last_was_thinking: false,
            commands: vec![
                "/help".into(),
                "/quit".into(),
                "/cd".into(),
                "/cd_default".into(),
                "/claude".into(),
                "/pwd".into(),
                "/ll".into(),
                "/mkdir".into(),
                "/show-thinking-toggle".into(),
                "/show-thinking".into(),
                "/hide-thinking".into(),
            ],
            completion_matches: Vec::new(),
            completion_index: 0,
            last_input_for_completion: String::new(),
        }
    }

    /// Return the suffix of the first matching command for inline hint display.
    fn compute_inline_hint(&self) -> Option<String> {
        if self.input.is_empty() || !self.input.starts_with('/') {
            return None;
        }
        self.commands
            .iter()
            .find(|cmd| cmd.starts_with(&self.input) && cmd.len() > self.input.len())
            .map(|cmd| cmd[self.input.len()..].to_string())
    }

    fn prompt_prefix(&self) -> String {
        let dir = if self.work_dir.is_empty() {
            std::env::current_dir()
                .map(|p| {
                    let s = p.to_string_lossy().to_string();
                    let parts: Vec<&str> = s.split('/').collect();
                    if parts.len() > 2 {
                        parts[parts.len() - 2..].join("/")
                    } else {
                        s
                    }
                })
                .unwrap_or_else(|_| "~".to_string())
        } else {
            let parts: Vec<&str> = self.work_dir.split('/').collect();
            if parts.len() > 2 {
                parts[parts.len() - 2..].join("/")
            } else {
                self.work_dir.clone()
            }
        };

        if self.session_active {
            format!("\u{1f4ac} {} \u{25b6} ", dir)
        } else {
            format!("\u{25cb} {} > ", dir)
        }
    }

    fn prompt_display_width(&self) -> usize {
        let prefix = self.prompt_prefix();
        UnicodeWidthStr::width(prefix.as_str())
    }

    fn add_message(&mut self, role: MsgRole, text: &str) {
        if text.trim().is_empty() && role == MsgRole::System {
            return;
        }
        self.messages.push(ChatMessage::new(role, text));
    }

    fn update_last_message(&mut self, role: MsgRole, text: &str) {
        if text.is_empty() {
            return;
        }
        match self.messages.last_mut() {
            Some(msg) if msg.role == role => msg.append(text),
            _ => self.add_message(role, text),
        }
    }
}

// ---------------------------------------------------------------------------
// Render
// ---------------------------------------------------------------------------

fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());

    // --- Message area ---
    let msg_block = Block::default().borders(Borders::NONE);
    let msg_area = msg_block.inner(chunks[0]);

    let mut all_lines: Vec<(Option<Color>, &str)> = Vec::new();
    for msg in &app.messages {
        let color = match msg.role {
            MsgRole::User => Some(Color::Gray),
            MsgRole::System | MsgRole::Thinking => Some(Color::DarkGray),
            MsgRole::Tool => Some(Color::Cyan),
            MsgRole::Error => Some(Color::Red),
            MsgRole::Claude => None,
        };
        for line in &msg.lines {
            all_lines.push((color, line.as_str()));
        }
    }

    let area_height = msg_area.height as usize;
    let total = all_lines.len();
    let visible_start = if app.scroll_offset >= total {
        total.saturating_sub(1)
    } else {
        total.saturating_sub(app.scroll_offset + area_height)
    };
    let visible_end = total.saturating_sub(app.scroll_offset);

    let visible_lines = if visible_start < visible_end {
        &all_lines[visible_start..visible_end]
    } else {
        &[]
    };

    let mut lines: Vec<Line> = visible_lines
        .iter()
        .map(|(color, text)| {
            let style = if let Some(c) = color {
                Style::default().fg(*c)
            } else {
                Style::default()
            };
            Line::from(Span::styled(text.to_string(), style))
        })
        .collect();

    // Bottom-align: pad with empty lines so content sits just above the input bar
    let padding = area_height.saturating_sub(lines.len());
    for _ in 0..padding {
        lines.insert(0, Line::from(""));
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .block(msg_block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, chunks[0]);

    // --- Input bar ---
    let prompt = app.prompt_prefix();
    let input_block = Block::default().borders(Borders::NONE);
    let before_cursor = if app.input_cursor <= app.input.len() {
        &app.input[..app.input_cursor]
    } else {
        &app.input[..]
    };
    let at_cursor = if app.input_cursor < app.input.len() {
        app.input.as_bytes()[app.input_cursor] as char
    } else {
        ' '
    };
    let after_cursor = if app.input_cursor < app.input.len() {
        &app.input[app.input_cursor + 1..]
    } else {
        ""
    };

    let hint = app.compute_inline_hint().unwrap_or_default();

    let input_line = Line::from(vec![
        Span::styled(prompt.clone(), Style::default().fg(Color::Gray)),
        Span::styled(before_cursor.to_string(), Style::default()),
        Span::styled(
            at_cursor.to_string(),
            Style::default().fg(Color::Black).bg(Color::White),
        ),
        Span::styled(after_cursor.to_string(), Style::default()),
        Span::styled(hint, Style::default().fg(Color::DarkGray)),
    ]);

    let input_para = Paragraph::new(input_line).block(input_block);
    f.render_widget(input_para, chunks[1]);

    // Position the terminal cursor using display width
    let cursor_x = (app.prompt_display_width() + UnicodeWidthStr::width(before_cursor)) as u16;
    f.set_cursor_position((cursor_x, chunks[1].y));
}

// ---------------------------------------------------------------------------
// Keyboard handling
// ---------------------------------------------------------------------------

enum KeyAction {
    Continue,
    Quit,
    Submit(String),
}

fn handle_key(key: &KeyEvent, app: &mut App) -> KeyAction {
    if key.kind != KeyEventKind::Press {
        return KeyAction::Continue;
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            KeyAction::Quit
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            KeyAction::Quit
        }
        KeyCode::Enter => {
            let text = std::mem::take(&mut app.input);
            app.input_cursor = 0;
            if text.is_empty() {
                KeyAction::Continue
            } else {
                KeyAction::Submit(text)
            }
        }
        KeyCode::Char(c) => {
            app.input.insert(app.input_cursor, c);
            app.input_cursor += c.len_utf8();
            KeyAction::Continue
        }
        KeyCode::Backspace => {
            if app.input_cursor > 0 {
                // Find the start byte of the previous char
                let mut prev = app.input_cursor - 1;
                while prev > 0 && !app.input.is_char_boundary(prev) {
                    prev -= 1;
                }
                app.input.remove(prev);
                app.input_cursor = prev;
            }
            KeyAction::Continue
        }
        KeyCode::Delete => {
            if app.input_cursor < app.input.len() {
                app.input.remove(app.input_cursor);
            }
            KeyAction::Continue
        }
        KeyCode::Left => {
            if app.input_cursor > 0 {
                let mut prev = app.input_cursor - 1;
                while prev > 0 && !app.input.is_char_boundary(prev) {
                    prev -= 1;
                }
                app.input_cursor = prev;
            }
            KeyAction::Continue
        }
        KeyCode::Right => {
            if app.input_cursor < app.input.len() {
                let mut next = app.input_cursor + 1;
                while next < app.input.len() && !app.input.is_char_boundary(next) {
                    next += 1;
                }
                app.input_cursor = next;
            }
            KeyAction::Continue
        }
        KeyCode::Home => {
            app.input_cursor = 0;
            KeyAction::Continue
        }
        KeyCode::End => {
            app.input_cursor = app.input.len();
            KeyAction::Continue
        }
        KeyCode::Esc => {
            app.input.clear();
            app.input_cursor = 0;
            KeyAction::Continue
        }
        KeyCode::Tab => {
            if app.input.starts_with('/') {
                // Recompute matches when input changed since last Tab
                if app.input != app.last_input_for_completion {
                    app.completion_matches = app
                        .commands
                        .iter()
                        .filter(|cmd| {
                            cmd.starts_with(&app.input) && cmd.len() > app.input.len()
                        })
                        .cloned()
                        .collect();
                    app.completion_index = 0;
                    app.last_input_for_completion = app.input.clone();
                }
                if !app.completion_matches.is_empty() {
                    app.input = app.completion_matches[app.completion_index].clone();
                    app.input_cursor = app.input.len();
                    app.completion_index =
                        (app.completion_index + 1) % app.completion_matches.len();
                }
            }
            KeyAction::Continue
        }
        KeyCode::Up => {
            app.scroll_offset += 1;
            KeyAction::Continue
        }
        KeyCode::Down => {
            app.scroll_offset = app.scroll_offset.saturating_sub(1);
            KeyAction::Continue
        }
        KeyCode::PageUp => {
            app.scroll_offset += 10;
            KeyAction::Continue
        }
        KeyCode::PageDown => {
            app.scroll_offset = app.scroll_offset.saturating_sub(10);
            KeyAction::Continue
        }
        _ => KeyAction::Continue,
    }
}

// ---------------------------------------------------------------------------
// Event loop — async helpers
// ---------------------------------------------------------------------------

struct SubmitResult {
    poll_claude: bool,
}

async fn process_submit(
    text: &str,
    app: &mut App,
    router: &CommandRouter,
    controller: &Arc<Mutex<ClaudeController>>,
) -> Result<SubmitResult> {
    // /quit when session active
    if text == "/quit" && app.session_active {
        {
            let ctrl = controller.lock().await;
            let _ = ctrl.stop_session().await;
        }
        app.session_active = false;
        app.add_message(MsgRole::System, &t!("cli.session_stopped"));
        return Ok(SubmitResult { poll_claude: false });
    }

    // /quit when session inactive — caller handles exit
    if text == "/quit" && !app.session_active {
        app.add_message(MsgRole::System, &t!("cli.goodbye"));
        return Ok(SubmitResult { poll_claude: false });
    }

    let response = router.handle(text).await;

    match response {
        Some(reply) => {
            app.add_message(MsgRole::System, &reply);

            if text.starts_with("/claude") {
                app.session_active = true;
                {
                    let ctrl = controller.lock().await;
                    app.work_dir = ctrl.get_work_dir().await;
                }
            }
            Ok(SubmitResult { poll_claude: false })
        }
        None => {
            app.add_message(MsgRole::User, text);
            app.needs_claude_response = true;
            Ok(SubmitResult { poll_claude: true })
        }
    }
}

/// Process a single Claude event, updating the app state.
/// Returns true when the response stream is done.
fn handle_claude_event(event: &ControllerEvent, app: &mut App) -> bool {
    match event {
        ControllerEvent::Text(text) => {
            app.last_was_thinking = false;
            app.update_last_message(MsgRole::Claude, text);
            false
        }
        ControllerEvent::Thinking(thinking) => {
            let display = if thinking.is_empty() {
                "\u{1f4ad} Thinking...".to_string()
            } else {
                format!("\u{1f4ad} Thinking... ({} chars)", thinking.len())
            };
            if !app.last_was_thinking {
                app.add_message(MsgRole::Thinking, &display);
            }
            app.last_was_thinking = true;
            false
        }
        ControllerEvent::ToolUse(name, input) => {
            let first_line = input.lines().next().unwrap_or("");
            let text = if first_line.is_empty() {
                format!("\u{1f527} Tool: {}", name)
            } else {
                format!("\u{1f527} Tool: {}\n  {}", name, first_line)
            };
            app.add_message(MsgRole::Tool, &text);
            false
        }
        ControllerEvent::ToolResult(content, is_error) => {
            if !content.is_empty() {
                let role = if *is_error { MsgRole::Error } else { MsgRole::System };
                app.add_message(role, content);
            }
            false
        }
        ControllerEvent::PermissionRequest(req_id, tool_name) => {
            let text = format!(
                "Permission Required: {} (id: {})\n  /allow or /deny",
                tool_name, req_id
            );
            app.add_message(MsgRole::System, &text);
            false
        }
        ControllerEvent::Error(err) => {
            app.add_message(MsgRole::Error, err);
            false
        }
        ControllerEvent::Done => true,
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_tui(
    controller: Arc<Mutex<ClaudeController>>,
    router: CommandRouter,
    _event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>>,
) -> Result<()> {
    let mut app = App::new();

    // Show banner
    let banner = format_banner();
    app.add_message(MsgRole::System, &banner);
    app.banner_shown = true;

    // Initial work dir
    {
        let ctrl = controller.lock().await;
        app.work_dir = ctrl.get_work_dir().await;
    }

    // --- Setup terminal ---
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    // --- Keyboard channel (blocking thread → async) ---
    let (key_tx, mut key_rx) = mpsc::unbounded_channel::<KeyEvent>();
    std::thread::spawn(move || loop {
        if TUI_EVENT_READER_PAUSED.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(10));
            continue;
        }
        if let Ok(true) = event::poll(std::time::Duration::from_millis(10)) {
            if let Ok(Event::Key(key)) = event::read() {
                if key_tx.send(key).is_err() {
                    break;
                }
            }
        }
    });

    // --- Initial render ---
    terminal.draw(|f| render(f, &app))?;

    // Helper: commands like /ll may disable raw mode; restore it before drawing.
    let ensure_raw = || {
        if let Ok(enabled) = crossterm::terminal::is_raw_mode_enabled() {
            if !enabled {
                let _ = crossterm::terminal::enable_raw_mode();
            }
        }
    };

    // --- Main loop ---
    let result: Result<()> = async {
        loop {
            if app.claude_busy {
                // Interleave keyboard input and Claude event polling.
                // The event_rx clone shares the underlying mpsc receiver,
                // so each clone receives the same event stream.
                let event_rx = {
                    let ctrl = controller.lock().await;
                    ctrl.event_rx_clone()
                };

                tokio::select! {
                    key_opt = key_rx.recv() => {
                        match key_opt {
                            Some(key) => {
                                match handle_key(&key, &mut app) {
                                    KeyAction::Quit => return Ok(()),
                                    KeyAction::Submit(text) => {
                                        let was_active = app.session_active;
                                        let submit = process_submit(
                                            &text, &mut app, &router, &controller,
                                        ).await?;
                                        if submit.poll_claude {
                                            app.claude_busy = true;
                                        }
                                        ensure_raw();
                                        let _ = terminal.clear();
                                        if text == "/quit" && !was_active {
                                            return Ok(());
                                        }
                                    }
                                    KeyAction::Continue => {}
                                }
                            }
                            None => return Ok(()),
                        }
                    }
                    event_opt = async {
                        let mut rx = event_rx.lock().await;
                        rx.recv().await
                    } => {
                        match event_opt {
                            Some(event) => {
                                let done = handle_claude_event(&event, &mut app);
                                if done {
                                    app.claude_busy = false;
                                    app.needs_claude_response = false;
                                    app.last_was_thinking = false;
                                }
                            }
                            None => {
                                // Channel closed — stop polling
                                app.claude_busy = false;
                                app.needs_claude_response = false;
                                app.last_was_thinking = false;
                            }
                        }
                    }
                }
            } else {
                // Only poll keyboard events when Claude is idle
                match key_rx.recv().await {
                    Some(key) => {
                        match handle_key(&key, &mut app) {
                            KeyAction::Submit(text) => {
                                let was_active = app.session_active;
                                let submit = process_submit(
                                    &text, &mut app, &router, &controller,
                                ).await?;

                                if submit.poll_claude {
                                    app.claude_busy = true;
                                }

                                // /quit when inactive exits
                                if text == "/quit" && !was_active {
                                    return Ok(());
                                }

                                ensure_raw();
                                let _ = terminal.clear();
                            }
                            KeyAction::Quit => return Ok(()),
                            KeyAction::Continue => {}
                        }
                    }
                    None => return Ok(()),
                }
            }

            terminal.draw(|f| render(f, &app))?;
        }
    }
    .await;

    // --- Cleanup ---
    let _ = terminal.show_cursor();
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;

    result
}
