use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
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
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use unicode_width::UnicodeWidthStr;

use crate::command::builtin::TUI_EVENT_READER_PAUSED;

use crate::claude::controller::{ClaudeController, QuestionItem};
use crate::claude::event_poller::{ClaudeEventPoller, EventPollSink};
use crate::cli::interactive::format_banner;
use crate::command::{CommandAction, CommandRouter};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::t_fmt;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub(crate) fn should_handle_key_event(key: &KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

/// Strip ANSI escape sequences so text renders cleanly in ratatui.
pub(crate) fn strip_ansi(s: &str) -> String {
    thread_local! {
        static RE: Regex = Regex::new("\x1b\\[[0-9;]*m").unwrap();
    }
    RE.with(|re| re.replace_all(s, "").to_string())
}

/// Split a string into display lines (respecting embedded newlines).
pub(crate) fn to_lines(s: &str) -> Vec<String> {
    if s.is_empty() {
        return vec![String::new()];
    }
    s.lines().map(|l| l.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Poll events from ClaudeEventPoller → main TUI loop
// ---------------------------------------------------------------------------

enum TuiPollEvent {
    Flush(String, bool),
    PermissionRequest(String, String),
    ConfirmRequest(String, String, Vec<String>),
    SelectRequest(String, String, Vec<String>),
    QuestionRequest(String, Vec<QuestionItem>),
    Done,
    PollerStopped,
}

struct TuiEventSink {
    tx: mpsc::UnboundedSender<TuiPollEvent>,
}

#[async_trait::async_trait]
impl EventPollSink for TuiEventSink {
    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()> {
        let _ = self.tx.send(TuiPollEvent::Flush(text.to_string(), is_done));
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        _input: Option<&serde_json::Value>,
    ) -> Result<()> {
        let _ = self.tx.send(TuiPollEvent::PermissionRequest(
            request_id.to_string(),
            tool_name.to_string(),
        ));
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        let _ = self.tx.send(TuiPollEvent::ConfirmRequest(
            request_id.to_string(),
            prompt.to_string(),
            options.to_vec(),
        ));
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        let _ = self.tx.send(TuiPollEvent::SelectRequest(
            request_id.to_string(),
            prompt.to_string(),
            options.to_vec(),
        ));
        Ok(())
    }

    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[QuestionItem],
    ) -> Result<()> {
        let _ = self.tx.send(TuiPollEvent::QuestionRequest(
            request_id.to_string(),
            questions.to_vec(),
        ));
        Ok(())
    }
}

/// Spawn one background poller for the active Claude session.
///
/// It stays alive across turns so multiple user messages in the same session
/// share a single receiver. Starting more than one poller races on the
/// controller's event channel and can make later Claude chunks disappear.
fn spawn_poller_task(
    controller: Arc<Mutex<ClaudeController>>,
    poll_tx: mpsc::UnboundedSender<TuiPollEvent>,
) {
    tokio::spawn(async move {
        loop {
            let poller = {
                let ctrl = controller.lock().await;
                if !ctrl.is_session_active().await {
                    break;
                }
                ClaudeEventPoller::from_controller(&*ctrl)
            };

            let mut sink = TuiEventSink {
                tx: poll_tx.clone(),
            };
            if let Err(e) = poller.run(&mut sink).await {
                tracing::warn!("[TUI] Poller error: {}", e);
            }

            // Notify main loop that this response stream is done
            let _ = poll_tx.send(TuiPollEvent::Done);

            // If session is still active, loop and wait for the next response
            let still_active = {
                let ctrl = controller.lock().await;
                ctrl.is_session_active().await
            };
            if !still_active {
                break;
            }
        }
        let _ = poll_tx.send(TuiPollEvent::PollerStopped);
    });
}

// ---------------------------------------------------------------------------
// Message model
// ---------------------------------------------------------------------------

#[derive(Clone, PartialEq, Debug)]
pub(crate) enum MsgRole {
    User,
    Claude,
    System,
}

#[derive(Clone, Debug)]
pub(crate) struct ChatMessage {
    pub(crate) role: MsgRole,
    pub(crate) lines: Vec<String>,
}

impl ChatMessage {
    pub(crate) fn new(role: MsgRole, text: &str) -> Self {
        Self {
            role,
            lines: to_lines(&strip_ansi(text)),
        }
    }

    pub(crate) fn append(&mut self, text: &str) {
        let clean = strip_ansi(text);
        let mut parts = clean.split('\n');
        let first = parts.next().unwrap_or("");

        match self.lines.last_mut() {
            Some(last) => last.push_str(first),
            None => self.lines.push(first.to_string()),
        }

        for part in parts {
            self.lines.push(part.to_string());
        }
    }
}
// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

pub(crate) struct App {
    pub(crate) messages: Vec<ChatMessage>,
    pub(crate) input: String,
    pub(crate) input_cursor: usize,
    pub(crate) scroll_offset: usize,
    pub(crate) claude_busy: bool,
    pub(crate) needs_claude_response: bool,
    pub(crate) session_active: bool,
    pub(crate) banner_shown: bool,
    /// Track consecutive thinking events for dedup.
    pub(crate) last_was_thinking: bool,
    /// Available slash commands for Tab completion.
    pub(crate) commands: Vec<String>,
    /// Current completion match list (recomputed on input change).
    pub(crate) completion_matches: Vec<String>,
    /// Index within completion_matches for cycling.
    pub(crate) completion_index: usize,
    /// Last input that triggered a completion (to detect change).
    pub(crate) last_input_for_completion: String,
    /// ID of the implicit TUI ChannelSession.
    pub(crate) channel_id: String,
    /// Whether a Claude event poller is already attached to this session.
    pub(crate) poller_running: bool,
}

impl App {
    pub(crate) fn new(channel_id: String) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            input_cursor: 0,
            scroll_offset: 0,
            claude_busy: false,
            needs_claude_response: false,
            session_active: false,
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
                "/show-thinking".into(),
                "/hide-thinking".into(),
                "/claude-history".into(),
            ],
            completion_matches: Vec::new(),
            completion_index: 0,
            last_input_for_completion: String::new(),
            channel_id,
            poller_running: false,
        }
    }

    /// Return the suffix of the first matching command for inline hint display.
    pub(crate) fn compute_inline_hint(&self) -> Option<String> {
        if self.input.is_empty() || !self.input.starts_with('/') {
            return None;
        }
        self.commands
            .iter()
            .find(|cmd| cmd.starts_with(&self.input) && cmd.len() > self.input.len())
            .map(|cmd| cmd[self.input.len()..].to_string())
    }

    pub(crate) fn prompt_prefix(&self) -> String {
        if self.session_active {
            "\u{1f4ac} \u{25b6} ".to_string()
        } else {
            "\u{25cb} > ".to_string()
        }
    }

    pub(crate) fn prompt_display_width(&self) -> usize {
        let prefix = self.prompt_prefix();
        UnicodeWidthStr::width(prefix.as_str())
    }

    pub(crate) fn add_message(&mut self, role: MsgRole, text: &str) {
        if text.trim().is_empty() && role == MsgRole::System {
            return;
        }
        self.messages.push(ChatMessage::new(role, text));
    }

    pub(crate) fn update_last_message(&mut self, role: MsgRole, text: &str) {
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
            MsgRole::System => Some(Color::DarkGray),
            MsgRole::Claude => None,
        };
        for line in &msg.lines {
            all_lines.push((color, line.as_str()));
        }
    }
    if app.claude_busy && app.needs_claude_response {
        all_lines.push((Some(Color::DarkGray), "..."));
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
    let mut cursor_idx = app.input_cursor.min(app.input.len());
    while cursor_idx > 0 && !app.input.is_char_boundary(cursor_idx) {
        cursor_idx -= 1;
    }
    let before_cursor = &app.input[..cursor_idx];
    let (at_cursor, after_cursor) = if cursor_idx < app.input.len() {
        let mut chars = app.input[cursor_idx..].chars();
        let ch = chars.next().unwrap_or(' ');
        let next_idx = cursor_idx + ch.len_utf8();
        (ch.to_string(), &app.input[next_idx..])
    } else {
        (" ".to_string(), "")
    };

    let hint = app.compute_inline_hint().unwrap_or_default();

    let input_line = Line::from(vec![
        Span::styled(prompt.clone(), Style::default().fg(Color::Gray)),
        Span::styled(before_cursor.to_string(), Style::default()),
        Span::styled(
            at_cursor,
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
    if !should_handle_key_event(key) {
        return KeyAction::Continue;
    }

    match key.code {
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,
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
                        .filter(|cmd| cmd.starts_with(&app.input) && cmd.len() > app.input.len())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandRouter;
    use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
    use crate::session::channel_model::{ClaudeSession as StoredClaudeSession, ClaudeSessionState};
    use crate::tests::helpers::TestEnv;
    use chrono::Utc;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn repeat_key_events_are_handled_like_presses() {
        let mut app = App::new("test-channel".to_string());
        app.messages
            .push(ChatMessage::new(MsgRole::System, "line one\nline two"));

        let key = KeyEvent::new_with_kind(KeyCode::Up, KeyModifiers::NONE, KeyEventKind::Repeat);
        let action = handle_key(&key, &mut app);

        assert!(matches!(action, KeyAction::Continue));
        assert_eq!(app.scroll_offset, 1);
    }

    #[test]
    fn render_input_with_multibyte_cursor_does_not_panic() {
        let mut app = App::new("test-channel".to_string());
        app.input = "你好".to_string();
        app.input_cursor = 0;

        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| render(f, &app)).unwrap();
    }

    #[tokio::test]
    async fn claude_history_resume_reuses_existing_tui_session_record() {
        let env = TestEnv::new();
        crate::db::init_schema().unwrap();
        let work_dir = env.home().join("resume-project");
        std::fs::create_dir_all(&work_dir).unwrap();
        let channel = GLOBAL_CHANNEL_SESSIONS
            .get_or_create_platform_channel("tui", "tui-history", work_dir.to_str().unwrap())
            .await;
        let now = Utc::now();
        let stored = StoredClaudeSession {
            id: "stored-session".to_string(),
            channel_session_id: channel.id.clone(),
            title: "TUI Session".to_string(),
            work_dir: work_dir.to_string_lossy().to_string(),
            active: false,
            state: ClaudeSessionState::Stopped,
            claude_session_id: Some("resume-session-id".to_string()),
            created_at: now,
            stopped_at: Some(now),
            updated_at: Some(now),
        };
        crate::db::insert_claude_session(&stored);
        GLOBAL_CHANNEL_SESSIONS.load_from_db();

        let controller = Arc::new(Mutex::new(ClaudeController::new(
            env.fake_claude_config(),
            false,
        )));
        let router = CommandRouter::new(controller.clone(), work_dir.to_str().unwrap());
        let mut app = App::new(channel.id.clone());

        process_submit("/claude-history 1", &mut app, &router, &controller)
            .await
            .unwrap();

        let sessions = GLOBAL_CHANNEL_SESSIONS.list_claude_sessions_by_channel(&channel.id, None);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "stored-session");
        assert!(sessions[0].active);
        assert_eq!(
            sessions[0].claude_session_id.as_deref(),
            Some("resume-session-id")
        );
        assert!(app.session_active);

        let ctrl = controller.lock().await;
        ctrl.stop_session().await.unwrap();
    }

    #[test]
    fn render_shows_ellipsis_while_waiting_for_first_claude_output() {
        let mut app = App::new("test-channel".to_string());
        app.add_message(MsgRole::User, "hello");
        app.claude_busy = true;
        app.needs_claude_response = true;

        let backend = TestBackend::new(40, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| render(f, &app)).unwrap();
        let rendered =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut acc, cell| {
                    acc.push_str(cell.symbol());
                    acc
                });

        assert!(rendered.contains("..."));
        assert_eq!(app.messages.len(), 1);
    }

    #[test]
    fn first_claude_chunk_replaces_transient_waiting_display() {
        let mut app = App::new("test-channel".to_string());
        app.add_message(MsgRole::User, "hello");
        app.needs_claude_response = true;

        let done = handle_poll_event(
            &TuiPollEvent::Flush("real answer".to_string(), false),
            &mut app,
        );

        assert!(!done);
        assert!(!app.needs_claude_response);
        assert_eq!(app.messages.len(), 2);
        assert_eq!(app.messages[1].role, MsgRole::Claude);
        assert_eq!(app.messages[1].lines, vec!["real answer"]);
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
    let action = router.route(text).await;

    match action {
        CommandAction::NoOp => Ok(SubmitResult { poll_claude: false }),

        CommandAction::Reply(msg) => {
            app.add_message(MsgRole::System, &msg);
            Ok(SubmitResult { poll_claude: false })
        }

        CommandAction::StopSession => {
            GLOBAL_CHANNEL_SESSIONS
                .refresh_active_controller_session(&app.channel_id, controller)
                .await;
            if let Some(reply) = router.execute(CommandAction::StopSession).await {
                app.add_message(MsgRole::System, &reply);
            }
            let _ = GLOBAL_CHANNEL_SESSIONS
                .stop_channel_session(&app.channel_id)
                .await;
            app.session_active = false;
            Ok(SubmitResult { poll_claude: false })
        }

        CommandAction::StartSession { work_dir, args } => {
            let result = router
                .execute(CommandAction::StartSession {
                    work_dir: work_dir.clone(),
                    args,
                })
                .await;
            if let Some(ref reply) = result {
                app.add_message(MsgRole::System, reply);
            }
            if GLOBAL_CHANNEL_SESSIONS
                .record_active_controller_session(&app.channel_id, "TUI Session", controller)
                .await?
                .is_some()
            {
                app.session_active = true;
                app.poller_running = false;
            }
            Ok(SubmitResult { poll_claude: false })
        }

        CommandAction::ShowClaudeHistory { arg } => {
            if let Some(reply) = router
                .execute(CommandAction::ShowClaudeHistory { arg })
                .await
            {
                app.add_message(MsgRole::System, &reply);
            }

            let pending_record_id = {
                let ctrl = controller.lock().await;
                ctrl.take_pending_resume_record_id().await
            };
            if let Some(record_id) = pending_record_id {
                match GLOBAL_CHANNEL_SESSIONS
                    .resume_claude_session_with_controller(&record_id, controller)
                    .await
                {
                    Ok(_) => {
                        app.session_active = true;
                        app.poller_running = false;
                    }
                    Err(e) => {
                        app.add_message(
                            MsgRole::System,
                            &t_fmt!("builtin.failed_start_claude", ERR = e),
                        );
                    }
                }
                return Ok(SubmitResult { poll_claude: false });
            }

            let has_pending_resume = {
                let ctrl = controller.lock().await;
                ctrl.has_pending_resume_session_id().await
            };
            if has_pending_resume {
                let result = router
                    .execute(CommandAction::StartSession {
                        work_dir: None,
                        args: Vec::new(),
                    })
                    .await;
                if let Some(ref reply) = result {
                    app.add_message(MsgRole::System, reply);
                }
                if GLOBAL_CHANNEL_SESSIONS
                    .record_active_controller_session(&app.channel_id, "TUI Session", controller)
                    .await?
                    .is_some()
                {
                    app.session_active = true;
                    app.poller_running = false;
                }
            }

            Ok(SubmitResult { poll_claude: false })
        }

        CommandAction::ChangeDir(_) | CommandAction::ChangeDirDefault => {
            let result = router.execute(action.clone()).await;
            if let Some(ref reply) = result {
                app.add_message(MsgRole::System, reply);
            }
            let ctrl = controller.lock().await;
            let wd = ctrl.get_work_dir().await;
            drop(ctrl);
            if !wd.is_empty() {
                let _ = GLOBAL_CHANNEL_SESSIONS
                    .switch_work_dir(&app.channel_id, PathBuf::from(wd))
                    .await;
            }
            Ok(SubmitResult { poll_claude: false })
        }

        CommandAction::ForwardToClaude(msg) => {
            match GLOBAL_CHANNEL_SESSIONS
                .send_to_controller(controller, &msg)
                .await
            {
                Ok(()) => {
                    app.add_message(MsgRole::User, text);
                    app.needs_claude_response = true;
                    Ok(SubmitResult { poll_claude: true })
                }
                Err(e) => {
                    app.add_message(MsgRole::System, &t_fmt!("forward.failed_send", ERR = e));
                    Ok(SubmitResult { poll_claude: false })
                }
            }
        }

        other => {
            if let Some(reply) = router.execute(other).await {
                app.add_message(MsgRole::System, &reply);
            }
            Ok(SubmitResult { poll_claude: false })
        }
    }
}

/// Process a single poll event from the ClaudeEventPoller, updating app state.
/// Returns true when the response stream is done.
fn handle_poll_event(event: &TuiPollEvent, app: &mut App) -> bool {
    match event {
        TuiPollEvent::Flush(text, _is_done) => {
            app.last_was_thinking = false;
            app.needs_claude_response = false;
            app.update_last_message(MsgRole::Claude, text);
            false
        }
        TuiPollEvent::PermissionRequest(request_id, tool_name) => {
            let text = crate::t_fmt!("tui.permission_required", NAME = tool_name, ID = request_id);
            app.add_message(MsgRole::System, &text);
            false
        }
        TuiPollEvent::ConfirmRequest(request_id, prompt, options) => {
            let text = crate::t_fmt!(
                "tui.confirm_request",
                ID = request_id,
                PROMPT = prompt,
                OPTIONS = format!("{:?}", options)
            );
            app.add_message(MsgRole::System, &text);
            false
        }
        TuiPollEvent::SelectRequest(request_id, prompt, options) => {
            let text = crate::t_fmt!(
                "tui.select_request",
                ID = request_id,
                PROMPT = prompt,
                OPTIONS = format!("{:?}", options)
            );
            app.add_message(MsgRole::System, &text);
            false
        }
        TuiPollEvent::QuestionRequest(request_id, questions) => {
            let mut text = crate::t_fmt!("tui.questions_title", ID = request_id);
            for q in questions {
                text.push_str(&crate::t_fmt!(
                    "tui.question_item",
                    HEADER = q.header,
                    QUESTION = q.question
                ));
                for opt in &q.options {
                    text.push_str(&crate::t_fmt!(
                        "tui.question_option",
                        LABEL = opt.label,
                        DESCRIPTION = opt.description
                    ));
                }
            }
            app.add_message(MsgRole::System, &text);
            false
        }
        TuiPollEvent::Done => true,
        TuiPollEvent::PollerStopped => {
            app.poller_running = false;
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_tui(
    controller: Arc<Mutex<ClaudeController>>,
    router: CommandRouter,
    channel_id: String,
) -> Result<()> {
    let mut app = App::new(channel_id);
    {
        let ctrl = controller.lock().await;
        if ctrl.is_session_active().await {
            app.session_active = true;
        }
    }

    // Show banner
    let banner = format_banner();
    app.add_message(MsgRole::System, &banner);
    app.banner_shown = true;

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
                if should_handle_key_event(&key) && key_tx.send(key).is_err() {
                    break;
                }
            }
        }
    });

    // --- Poll channel (ClaudeEventPoller → main loop) ---
    let (poll_tx, mut poll_rx) = mpsc::unbounded_channel::<TuiPollEvent>();

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
                                            if !app.poller_running {
                                                app.poller_running = true;
                                                spawn_poller_task(controller.clone(), poll_tx.clone());
                                            }
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
                    poll_opt = poll_rx.recv() => {
                        match poll_opt {
                            Some(event) => {
                                let done = handle_poll_event(&event, &mut app);
                                if done {
                                    app.claude_busy = false;
                                    app.needs_claude_response = false;
                                    app.last_was_thinking = false;
                                }
                            }
                            None => {
                                app.claude_busy = false;
                                app.needs_claude_response = false;
                                app.last_was_thinking = false;
                            }
                        }
                    }
                }
            } else {
                tokio::select! {
                    key_opt = key_rx.recv() => {
                        match key_opt {
                            Some(key) => {
                                match handle_key(&key, &mut app) {
                                    KeyAction::Submit(text) => {
                                        let was_active = app.session_active;
                                        let submit = process_submit(
                                            &text, &mut app, &router, &controller,
                                        ).await?;

                                        if submit.poll_claude {
                                            app.claude_busy = true;
                                            if !app.poller_running {
                                                app.poller_running = true;
                                                spawn_poller_task(controller.clone(), poll_tx.clone());
                                            }
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
                    poll_opt = poll_rx.recv() => {
                        match poll_opt {
                            Some(event) => {
                                let done = handle_poll_event(&event, &mut app);
                                if done {
                                    app.claude_busy = false;
                                    app.needs_claude_response = false;
                                    app.last_was_thinking = false;
                                }
                            }
                            None => {
                                app.claude_busy = false;
                                app.needs_claude_response = false;
                                app.last_was_thinking = false;
                                app.poller_running = false;
                            }
                        }
                    }
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
