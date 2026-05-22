use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::validate::Validator;
use rustyline::{
    completion::{Completer, Pair},
    hint::Hinter,
    Context, Helper,
};
#[cfg(test)]
use rustyline::history::MemHistory;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
#[cfg(test)]
use crate::claude::controller::ControllerEvent;
use crate::config::loader::ConfigLoader;
use crate::t;

// ANSI color/style codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";

// ---------------------------------------------------------------------------
// Formatting helpers – pure functions returning display strings.
// These are used by both the real terminal and unit tests.
// ---------------------------------------------------------------------------

pub fn format_banner() -> String {
    t!("cli.banner").to_string()
}

pub fn format_thinking_collapsed(thinking: &str) -> String {
    let _ = thinking;
    format!(
        "{}\u{1F4AD} {} {} {}{}{}",
        GRAY, t!("cli.thinking"), RESET, DIM, t!("cli.press_expand"), RESET
    )
}

pub fn format_user_echo(text: &str) -> String {
    let mut result = String::new();
    for line in text.lines() {
        result.push_str(&format!("{}  {}>{} {}{}\n", GRAY, DIM, RESET, line, RESET));
    }
    result
}

pub fn format_tool_use_inline(name: &str, input: &str) -> String {
    let first_line = input.lines().next().unwrap_or("");
    let mut result = format!("{}\u{1F527} {} {}{}{}\n", CYAN, t!("cli.tool_label"), BOLD, name, RESET);
    if !first_line.is_empty() {
        result.push_str(&format!("  {}{}{}\n", DIM, first_line, RESET));
    }
    result
}

pub fn format_tool_result(content: &str, is_error: bool) -> String {
    if content.is_empty() {
        return String::new();
    }
    let mut result = String::new();
    for line in content.lines() {
        if is_error {
            result.push_str(&format!("  {}> {}{}\n", RED, line, RESET));
        } else {
            result.push_str(&format!("  {}{}{}\n", DIM, line, RESET));
        }
    }
    result
}

pub fn format_permission_request(req_id: &str, tool_name: &str) -> String {
    let mut result = format!(
        "{}\u{26A0}\u{FE0F}  {}{}  {}: {}{}{}  {}: {}{}{}\n",
        YELLOW, t!("cli.permission_required"), RESET, t!("cli.tool_label"), BOLD, tool_name, RESET, t!("cli.request_id"), DIM, req_id, RESET
    );
    result.push_str(&format!(
        "   {}\n",
        t!("cli.allow_deny_hint")
    ));
    result
}

pub fn format_error(err: &str) -> String {
    let mut result = String::new();
    for line in err.lines() {
        result.push_str(&format!("{}\u{274C} {}{}\n", RED, line, RESET));
    }
    result
}

pub fn format_response(response: &str) -> String {
    let mut result = String::new();
    if !response.is_empty() {
        for line in response.lines() {
            result.push_str(&format!("{}\u{25B6} {}{}\n", BLUE, line, RESET));
        }
    }
    result
}

pub fn format_interrupt() -> String {
    format!("{}^C{}\n", GRAY, RESET)
}

pub fn format_eof() -> String {
    format!("{}^D{}\n", GRAY, RESET)
}

pub fn format_readline_error(err: &ReadlineError) -> String {
    format!("{}Error: {:?}{}\n", RED, err, RESET)
}

pub fn format_goodbye() -> String {
    format!("\n{}{}{}\n", GREEN, t!("cli.goodbye"), RESET)
}

pub fn format_prompt(work_dir: &str, active: bool) -> String {
    let dir = if work_dir.is_empty() {
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
        let parts: Vec<&str> = work_dir.split('/').collect();
        if parts.len() > 2 {
            parts[parts.len() - 2..].join("/")
        } else {
            work_dir.to_string()
        }
    };

    if active {
        // Chat input-box look when a Claude session is alive
        format!("{}💬{} {} {}▶ ", GREEN, RESET, dim_text(&dir), BOLD)
    } else {
        format!("{}○{} {} {}> ", GRAY, RESET, dim_text(&dir), BOLD)
    }
}

fn dim_text(s: &str) -> String {
    format!("{}{}{}", GRAY, s, RESET)
}

// ---------------------------------------------------------------------------
// CommandHelper – provides tab-completion and inline hints for / commands.
// ---------------------------------------------------------------------------

struct CommandHelper {
    commands: Vec<(String, String)>,
}

impl CommandHelper {
    fn new() -> Self {
        Self {
            commands: vec![
                ("/help".into(), t!("cli.help_desc").to_string()),
                ("/quit".into(), t!("cli.quit_desc").to_string()),
                ("/cd".into(), t!("cli.cd_desc").to_string()),
                ("/claude".into(), t!("cli.claude_desc").to_string()),
                ("/pwd".into(), t!("cli.pwd_desc").to_string()),
                ("/ll".into(), t!("cli.ll_desc").to_string()),
            ],
        }
    }
}

impl Completer for CommandHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        if !line.starts_with('/') {
            return Ok((0, Vec::new()));
        }

        let matches: Vec<Pair> = self
            .commands
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(line))
            .map(|(cmd, desc)| Pair {
                display: format!("{:<14} {}", cmd, desc),
                replacement: cmd.clone(),
            })
            .collect();

        Ok((0, matches))
    }
}

impl Hinter for CommandHelper {
    type Hint = String;

    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<String> {
        if line.is_empty() || !line.starts_with('/') {
            return None;
        }

        let first = self
            .commands
            .iter()
            .find(|(cmd, _)| cmd.starts_with(line) && cmd != line)?;

        Some(first.0[line.len()..].to_string())
    }
}

impl Highlighter for CommandHelper {}
impl Validator for CommandHelper {}
impl Helper for CommandHelper {}

// ---------------------------------------------------------------------------
// CliOutput – state machine that mirrors the real event listener.
// Collects every line that would be printed to the terminal.
// Only used in unit tests.
// ---------------------------------------------------------------------------

#[cfg(test)]
#[derive(Debug, Default)]
pub struct CliOutput {
    pub lines: Vec<String>,
    pub text_buffer: String,
    pub text_in_progress: bool,
}

#[cfg(test)]
impl CliOutput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Process a controller event exactly as the real terminal listener would.
    /// Returns `true` when a Done event is received (text stream ends).
    pub fn process_event(&mut self, event: &ControllerEvent) -> bool {
        match event {
            ControllerEvent::Text(text) => {
                self.text_buffer.push_str(text);
                self.text_in_progress = true;
                false
            }
            ControllerEvent::Thinking(thinking) => {
                self.flush_text();
                let line = if thinking.is_empty() {
                    "💭 Thinking...".to_string()
                } else {
                    format_thinking_collapsed(thinking)
                };
                if self.lines.last() != Some(&line) {
                    self.lines.push(line);
                }
                false
            }
            ControllerEvent::ToolUse(name, input) => {
                self.flush_text();
                self.lines.push(format_tool_use_inline(name, input));
                false
            }
            ControllerEvent::ToolResult(content, is_error) => {
                self.flush_text();
                let formatted = format_tool_result(content, *is_error);
                if !formatted.is_empty() {
                    self.lines.push(formatted);
                }
                false
            }
            ControllerEvent::PermissionRequest(req_id, tool_name) => {
                self.flush_text();
                self.lines.push(format_permission_request(req_id, tool_name));
                false
            }
            ControllerEvent::Error(err) => {
                self.flush_text();
                self.lines.push(format_error(err));
                false
            }
            ControllerEvent::Done => {
                self.flush_text();
                true
            }
        }
    }

    /// Feed a user command response (e.g. /help output) into the collector.
    pub fn feed_response(&mut self, response: &str) {
        let formatted = format_response(response);
        if !formatted.is_empty() {
            self.lines.push(formatted);
        }
    }

    fn flush_text(&mut self) {
        if self.text_in_progress {
            self.lines.push(std::mem::take(&mut self.text_buffer));
            self.text_in_progress = false;
        }
    }

    /// Drain collected lines, returning them and clearing the buffer.
    pub fn take_lines(&mut self) -> Vec<String> {
        self.flush_text();
        std::mem::take(&mut self.lines)
    }

    /// Peek at current collected output without draining.
    pub fn peek_output(&self) -> String {
        let mut result = self.lines.join("");
        if self.text_in_progress {
            result.push_str(&self.text_buffer);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Real terminal entry point
// ---------------------------------------------------------------------------

pub async fn run_interactive() -> Result<()> {
    let config = ConfigLoader::load()?;
    let controller = Arc::new(Mutex::new(ClaudeController::new(
        config.claude.clone(),
        config.show_thinking,
    )));
    {
        let ctrl = controller.lock().await;
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| ".".to_string());
        ctrl.init_work_dir(cwd).await;
    }
    let default_dir = &config.default_dir;
    let router = crate::command::router::CommandRouter::new(controller.clone(), default_dir);

    let event_rx = {
        let ctrl = controller.lock().await;
        ctrl.event_rx_clone()
    };

    crate::cli::tui::run_tui(controller, router, event_rx).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::controller::ControllerEvent;

    // ------------------------------------------------------------------
    // Format helper tests
    // ------------------------------------------------------------------

    #[test]
    fn test_format_banner_contains_mode() {
        let s = format_banner();
        assert!(s.contains("interactive mode"));
        assert!(s.contains("/help"));
    }

    #[test]
    fn test_format_thinking_collapsed() {
        let s = format_thinking_collapsed("some thought");
        assert!(s.contains("Thinking"));
        assert!(s.contains(GRAY));
        assert!(s.contains(RESET));
    }

    #[test]
    fn test_format_tool_use_inline() {
        let s = format_tool_use_inline("Bash", "{\"cmd\":\"ls\"}");
        assert!(s.contains("Tool:"));
        assert!(s.contains("Bash"));
        assert!(s.contains("{"));
    }

    #[test]
    fn test_format_tool_result_success() {
        let s = format_tool_result("file1\nfile2", false);
        assert!(s.contains("file1"));
        assert!(s.contains("file2"));
        assert!(!s.contains(">")); // not error style
    }

    #[test]
    fn test_format_tool_result_error() {
        let s = format_tool_result("not found", true);
        assert!(s.contains("> not found"));
        assert!(s.contains(RED));
    }

    #[test]
    fn test_format_tool_result_empty() {
        let s = format_tool_result("", false);
        assert!(s.is_empty());
    }

    #[test]
    fn test_format_permission_request() {
        let s = format_permission_request("req-1", "Bash");
        assert!(s.contains("Permission Required"));
        assert!(s.contains("Bash"));
        assert!(s.contains("req-1"));
        assert!(s.contains("/allow"));
        assert!(s.contains("/deny"));
    }

    #[test]
    fn test_format_error() {
        let s = format_error("oops");
        assert!(s.contains("oops"));
        assert!(s.contains(RED));
    }

    #[test]
    fn test_format_response() {
        let s = format_response("line1\nline2");
        assert!(s.contains("line1"));
        assert!(s.contains("line2"));
        assert!(s.contains(BLUE));
    }

    #[test]
    fn test_format_interrupt() {
        let s = format_interrupt();
        assert!(s.contains("^C"));
    }

    #[test]
    fn test_format_eof() {
        let s = format_eof();
        assert!(s.contains("^D"));
    }

    #[test]
    fn test_format_goodbye() {
        let s = format_goodbye();
        assert!(s.contains("Goodbye"));
        assert!(s.contains(GREEN));
    }

    #[test]
    fn test_format_prompt_active() {
        let s = format_prompt("/Users/alice/Workspace", true);
        assert!(s.contains("💬"));
        assert!(s.contains("▶"));
    }

    #[test]
    fn test_format_prompt_inactive() {
        let s = format_prompt("", false);
        assert!(s.contains("○"));
        assert!(s.contains(">"));
    }

    // ------------------------------------------------------------------
    // CliOutput state-machine tests – these verify the full event flow.
    // ------------------------------------------------------------------

    #[test]
    fn test_cli_output_text_stream_and_done() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("Hello ".into()));
        out.process_event(&ControllerEvent::Text("world".into()));
        assert!(out.text_in_progress);
        let done = out.process_event(&ControllerEvent::Done);
        assert!(done);
        assert!(!out.text_in_progress);
        assert_eq!(out.lines, vec!["Hello world"]);
    }

    #[test]
    fn test_cli_output_thinking_flushes_text() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("Before ".into()));
        out.process_event(&ControllerEvent::Thinking("thinking...".into()));
        assert!(!out.text_in_progress);
        assert!(out.lines[0].contains("Before "));
        assert!(out.lines[1].contains("Thinking"));
    }

    #[test]
    fn test_cli_output_tool_use_flushes_text() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("abc".into()));
        out.process_event(&ControllerEvent::ToolUse("Bash".into(), "{\"cmd\":\"ls\"}".into()));
        assert!(out.lines[0].contains("abc"));
        assert!(out.lines[1].contains("Tool:"));
    }

    #[test]
    fn test_cli_output_tool_result_flushes_text() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("pre".into()));
        out.process_event(&ControllerEvent::ToolResult("output".into(), false));
        assert!(out.lines[0].contains("pre"));
        assert!(out.lines[1].contains("output"));
    }

    #[test]
    fn test_cli_output_permission_request_flushes_text() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("pre".into()));
        out.process_event(&ControllerEvent::PermissionRequest("req-1".into(), "Bash".into()));
        assert!(out.lines[0].contains("pre"));
        assert!(out.lines[1].contains("Permission"));
    }

    #[test]
    fn test_cli_output_error_flushes_text() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("pre".into()));
        out.process_event(&ControllerEvent::Error("boom".into()));
        assert!(out.lines[0].contains("pre"));
        assert!(out.lines[1].contains("boom"));
    }

    #[test]
    fn test_cli_output_done_flushes_text() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("final".into()));
        let done = out.process_event(&ControllerEvent::Done);
        assert!(done);
        assert_eq!(out.lines, vec!["final"]);
    }

    #[test]
    fn test_cli_output_feed_response() {
        let mut out = CliOutput::new();
        out.feed_response("/help output");
        assert!(out.lines[0].contains("/help output"));
        assert!(out.lines[0].contains(BLUE));
    }

    #[test]
    fn test_cli_output_take_lines_drains_buffer() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("a".into()));
        out.process_event(&ControllerEvent::Done);
        let lines = out.take_lines();
        assert_eq!(lines, vec!["a"]);
        assert!(out.lines.is_empty());
    }

    #[test]
    fn test_cli_output_peek_does_not_drain() {
        let mut out = CliOutput::new();
        out.process_event(&ControllerEvent::Text("peek".into()));
        out.process_event(&ControllerEvent::Done);
        let _ = out.peek_output();
        assert_eq!(out.lines.len(), 1);
    }

    #[test]
    fn test_cli_output_full_conversation_flow() {
        let mut out = CliOutput::new();

        // Simulate a realistic Claude response sequence
        out.process_event(&ControllerEvent::Text("Let me ".into()));
        out.process_event(&ControllerEvent::Text("check ".into()));
        out.process_event(&ControllerEvent::Thinking("hmm...".into()));
        out.process_event(&ControllerEvent::ToolUse("Bash".into(), "{\"cmd\":\"ls\"}".into()));
        out.process_event(&ControllerEvent::ToolResult("file.txt".into(), false));
        out.process_event(&ControllerEvent::Text("Here is ".into()));
        out.process_event(&ControllerEvent::Text("the result.".into()));
        out.process_event(&ControllerEvent::Done);

        let output = out.take_lines();
        assert!(output[0].contains("Let me check"));
        assert!(output[1].contains("Thinking"));
        assert!(output[2].contains("Tool:"));
        assert!(output[3].contains("file.txt"));
        assert!(output[4].contains("Here is the result."));
    }

    // ------------------------------------------------------------------
    // CommandHelper tests – completion and hint logic.
    // ------------------------------------------------------------------

    #[test]
    fn test_helper_complete_empty_returns_nothing() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let (pos, matches) = helper.complete("", 0, &Context::new(&hist)).unwrap();
        assert_eq!(pos, 0);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_helper_complete_slash_lists_all_commands() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let (pos, matches) = helper.complete("/", 1, &Context::new(&hist)).unwrap();
        assert_eq!(pos, 0);
        assert!(
            matches.len() >= 6,
            "should list all slash commands, got {}",
            matches.len()
        );
        assert!(matches.iter().any(|m| m.replacement == "/help"));
        assert!(matches.iter().any(|m| m.replacement == "/quit"));
        assert!(matches.iter().any(|m| m.replacement == "/claude"));
    }

    #[test]
    fn test_helper_complete_prefix_filters() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let (pos, matches) = helper.complete("/c", 2, &Context::new(&hist)).unwrap();
        assert_eq!(pos, 0);
        assert!(
            matches.iter().all(|m| m.replacement.starts_with("/c")),
            "all matches should start with /c"
        );
        assert!(matches.iter().any(|m| m.replacement == "/cd"));
        assert!(matches.iter().any(|m| m.replacement == "/claude"));
    }

    #[test]
    fn test_helper_complete_no_slash_returns_empty() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let (pos, matches) = helper.complete("hello", 5, &Context::new(&hist)).unwrap();
        assert_eq!(pos, 0);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_helper_hint_slash_prefix() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let hint = helper.hint("/he", 3, &Context::new(&hist));
        assert_eq!(hint, Some("lp".to_string()));
    }

    #[test]
    fn test_helper_hint_no_match() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let hint = helper.hint("/zzz", 4, &Context::new(&hist));
        assert_eq!(hint, None);
    }

    #[test]
    fn test_helper_hint_exact_match_no_hint() {
        let helper = CommandHelper::new();
        let hist = MemHistory::new();
        let hint = helper.hint("/help", 5, &Context::new(&hist));
        assert_eq!(hint, None);
    }
}
