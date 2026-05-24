// Extended tests for CLI TUI module (src/cli/tui.rs).
//
// Tests cover: App state, ChatMessage model, MsgRole display, strip_ansi,
// to_lines, prompt rendering, inline hints, command completion state,
// message management, and edge cases.

use crate::cli::tui::{App, ChatMessage, MsgRole, strip_ansi, to_lines};

// ---------------------------------------------------------------------------
// App::new - initial state
// ---------------------------------------------------------------------------

#[test]
fn test_app_new_state() {
    let app = App::new("test-channel-id".to_string());

    // Messages start empty
    assert!(app.messages.is_empty());
    // Input starts empty
    assert_eq!(app.input, "");
    // Cursor at position 0
    assert_eq!(app.input_cursor, 0);
    // Scroll offset 0
    assert_eq!(app.scroll_offset, 0);
    // Not busy
    assert!(!app.claude_busy);
    // No pending Claude response
    assert!(!app.needs_claude_response);
    // No active session
    assert!(!app.session_active);
    // Banner not shown
    assert!(!app.banner_shown);
    // Last was not thinking
    assert!(!app.last_was_thinking);
    // Commands are populated (10 builtins)
    assert_eq!(app.commands.len(), 11);
    assert!(app.commands.contains(&"/help".to_string()));
    assert!(app.commands.contains(&"/quit".to_string()));
    assert!(app.commands.contains(&"/claude".to_string()));
    // Completion state initialized
    assert!(app.completion_matches.is_empty());
    assert_eq!(app.completion_index, 0);
    assert_eq!(app.last_input_for_completion, "");
    // Channel ID stored
    assert_eq!(app.channel_id, "test-channel-id");
}

// ---------------------------------------------------------------------------
// strip_ansi
// ---------------------------------------------------------------------------

#[test]
fn test_strip_ansi_removes_color_codes() {
    assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
}

#[test]
fn test_strip_ansi_preserves_plain_text() {
    assert_eq!(strip_ansi("hello world"), "hello world");
    assert_eq!(strip_ansi("no ansi here"), "no ansi here");
}

#[test]
fn test_strip_ansi_empty_string() {
    assert_eq!(strip_ansi(""), "");
}

#[test]
fn test_strip_ansi_multiple_escape_sequences() {
    let input = "\x1b[1mBold\x1b[0m \x1b[4mUnderline\x1b[0m";
    assert_eq!(strip_ansi(input), "Bold Underline");
}

#[test]
fn test_strip_ansi_complex_sequence() {
    // CSI sequences can have multiple params separated by semicolons
    assert_eq!(strip_ansi("\x1b[38;5;196mred256\x1b[0m"), "red256");
}

// ---------------------------------------------------------------------------
// to_lines
// ---------------------------------------------------------------------------

#[test]
fn test_to_lines_splits_on_newline() {
    let result = to_lines("line1\nline2\nline3");
    assert_eq!(result, vec!["line1", "line2", "line3"]);
}

#[test]
fn test_to_lines_empty_string_returns_vec_with_empty() {
    let result = to_lines("");
    assert_eq!(result, vec![""]);
}

#[test]
fn test_to_lines_single_line_no_newline() {
    let result = to_lines("single");
    assert_eq!(result, vec!["single"]);
}

#[test]
fn test_to_lines_trailing_newline() {
    // Rust's .lines() ignores the trailing empty string
    let result = to_lines("a\nb\n");
    assert_eq!(result, vec!["a", "b"]);
}

#[test]
fn test_to_lines_with_carriage_returns() {
    let result = to_lines("line1\r\nline2");
    // \r is part of the line content, not stripped by lines()
    assert!(result[0].ends_with('\r') || result[0] == "line1");
}

// ---------------------------------------------------------------------------
// ChatMessage
// ---------------------------------------------------------------------------

#[test]
fn test_chat_message_new_user_role() {
    let msg = ChatMessage::new(MsgRole::User, "hello world");
    assert_eq!(msg.role, MsgRole::User);
    assert_eq!(msg.lines, vec!["hello world"]);
}

#[test]
fn test_chat_message_new_strips_ansi() {
    let msg = ChatMessage::new(MsgRole::Claude, "\x1b[32mgreen\x1b[0m");
    assert_eq!(msg.lines, vec!["green"]);
}

#[test]
fn test_chat_message_new_multiline() {
    let msg = ChatMessage::new(MsgRole::System, "line1\nline2");
    assert_eq!(msg.lines, vec!["line1", "line2"]);
}

#[test]
fn test_chat_message_append_to_existing() {
    let mut msg = ChatMessage::new(MsgRole::Claude, "Hello");
    msg.append(" world");
    assert_eq!(msg.lines, vec!["Hello world"]);
}

#[test]
fn test_chat_message_append_strips_ansi() {
    let mut msg = ChatMessage::new(MsgRole::Claude, "Hello");
    msg.append(" \x1b[1mbold\x1b[0m");
    assert_eq!(msg.lines, vec!["Hello bold"]);
}

#[test]
fn test_chat_message_append_without_existing_lines() {
    // This shouldn't happen in normal usage but verify no panic
    let mut msg = ChatMessage {
        role: MsgRole::Claude,
        lines: vec![],
    };
    msg.append("fresh");
    assert_eq!(msg.lines, vec!["fresh"]);
}

#[test]
fn test_msg_role_display() {
    // Verify the enum variants exist and compare correctly
    assert_eq!(MsgRole::User, MsgRole::User);
    assert_eq!(MsgRole::Claude, MsgRole::Claude);
    assert_eq!(MsgRole::System, MsgRole::System);
    assert_ne!(MsgRole::User, MsgRole::Claude);
    assert_ne!(MsgRole::User, MsgRole::System);
}

// ---------------------------------------------------------------------------
// App::prompt_prefix
// ---------------------------------------------------------------------------

#[test]
fn test_app_prompt_prefix_inactive() {
    let app = App::new("ch1".to_string());
    assert!(!app.session_active);
    // Inactive prompt: "○ >"
    let prefix = app.prompt_prefix();
    assert!(!prefix.is_empty());
    assert!(!prefix.contains('\u{1f4ac}')); // no speech balloon emoji
}

#[test]
fn test_app_prompt_prefix_active() {
    let mut app = App::new("ch1".to_string());
    app.session_active = true;
    // Active prompt: "💬 ▶"
    let prefix = app.prompt_prefix();
    assert!(prefix.contains('\u{1f4ac}')); // speech balloon
    assert!(prefix.contains('\u{25b6}')); // play button
}

#[test]
fn test_app_prompt_display_width_nonzero() {
    let app = App::new("ch1".to_string());
    let width = app.prompt_display_width();
    assert!(width > 0);
}

// ---------------------------------------------------------------------------
// App::add_message
// ---------------------------------------------------------------------------

#[test]
fn test_app_add_message_appends_to_list() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::User, "hello");
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].role, MsgRole::User);
    assert_eq!(app.messages[0].lines, vec!["hello"]);
}

#[test]
fn test_app_add_message_skips_empty_system() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::System, "   ");
    assert!(app.messages.is_empty());
}

#[test]
fn test_app_add_message_accepts_empty_user() {
    let mut app = App::new("ch1".to_string());
    // Empty user messages are NOT skipped (only system messages are)
    app.add_message(MsgRole::User, "");
    assert_eq!(app.messages.len(), 1);
}

#[test]
fn test_app_add_message_multiple() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::User, "first");
    app.add_message(MsgRole::Claude, "second");
    app.add_message(MsgRole::System, "third");
    assert_eq!(app.messages.len(), 3);
}

// ---------------------------------------------------------------------------
// App::update_last_message
// ---------------------------------------------------------------------------

#[test]
fn test_app_update_last_message_appends_when_same_role() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::Claude, "Hello");
    app.update_last_message(MsgRole::Claude, " world");
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].lines, vec!["Hello world"]);
}

#[test]
fn test_app_update_last_message_creates_new_on_role_mismatch() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::Claude, "Hello");
    app.update_last_message(MsgRole::User, "Hi back");
    assert_eq!(app.messages.len(), 2);
    assert_eq!(app.messages[1].role, MsgRole::User);
}

#[test]
fn test_app_update_last_message_skips_empty_text() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::Claude, "Hello");
    app.update_last_message(MsgRole::Claude, "");
    // No change since empty text is skipped
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].lines, vec!["Hello"]);
}

#[test]
fn test_app_update_last_message_empty_list_creates_new() {
    let mut app = App::new("ch1".to_string());
    app.update_last_message(MsgRole::Claude, "Hello");
    assert_eq!(app.messages.len(), 1);
}

// ---------------------------------------------------------------------------
// App::compute_inline_hint
// ---------------------------------------------------------------------------

#[test]
fn test_app_compute_inline_hint_no_input() {
    let app = App::new("ch1".to_string());
    assert_eq!(app.compute_inline_hint(), None);
}

#[test]
fn test_app_compute_inline_hint_non_slash_input() {
    let mut app = App::new("ch1".to_string());
    app.input = "hello".to_string();
    assert_eq!(app.compute_inline_hint(), None);
}

#[test]
fn test_app_compute_inline_hint_partial_slash_command() {
    let mut app = App::new("ch1".to_string());
    app.input = "/hel".to_string();
    let hint = app.compute_inline_hint();
    assert_eq!(hint, Some("p".to_string())); // completes to "/help"
}

#[test]
fn test_app_compute_inline_hint_exact_match_no_hint() {
    let mut app = App::new("ch1".to_string());
    app.input = "/help".to_string();
    let hint = app.compute_inline_hint();
    // Exact match with no longer alternative: no hint
    // /help is the only command starting with /help
    assert!(hint.is_none());
}

#[test]
fn test_app_compute_inline_hint_slash_claude() {
    let mut app = App::new("ch1".to_string());
    app.input = "/clau".to_string();
    let hint = app.compute_inline_hint();
    assert_eq!(hint, Some("de".to_string())); // completes to "/claude"
}

#[test]
fn test_app_compute_inline_hint_multiple_matches_returns_first() {
    let mut app = App::new("ch1".to_string());
    // "/cl" could match "/claude"
    // We have a deterministic set of commands, so first match in list order wins
    app.input = "/cl".to_string();
    let hint = app.compute_inline_hint();
    assert!(hint.is_some());
    // Should complete to "/claude" (first in list that starts with "/cl")
    assert_eq!(hint.unwrap(), "aude".to_string());
}

#[test]
fn test_app_compute_inline_hint_empty_slash() {
    let mut app = App::new("ch1".to_string());
    app.input = "/".to_string();
    let hint = app.compute_inline_hint();
    // "/" matches all commands, returns the first one's suffix
    assert!(hint.is_some());
}

// ---------------------------------------------------------------------------
// App message list: test scroll offset interaction
// ---------------------------------------------------------------------------

#[test]
fn test_app_messages_deduplicate_consecutive_thinking() {
    let mut app = App::new("ch1".to_string());
    app.add_message(MsgRole::Claude, "thinking...");
    app.add_message(MsgRole::Claude, "more thinking...");
    // Both are separate messages (no dedup in add_message)
    assert_eq!(app.messages.len(), 2);
}

#[test]
fn test_app_update_last_message_streaming_pattern() {
    // Simulates the streaming update pattern:
    // Claude sends chunks of text that get appended to the last message
    let mut app = App::new("ch1".to_string());
    app.update_last_message(MsgRole::Claude, "chunk1");
    assert_eq!(app.messages.len(), 1);
    app.update_last_message(MsgRole::Claude, "chunk2");
    assert_eq!(app.messages.len(), 1);
    app.update_last_message(MsgRole::Claude, "chunk3");
    // Correct: all chunks merged into one message
    assert_eq!(app.messages.len(), 1);
    assert_eq!(app.messages[0].lines, vec!["chunk1chunk2chunk3"]);
}

#[test]
fn test_app_update_last_message_streaming_interrupted_by_other_role() {
    let mut app = App::new("ch1".to_string());
    app.update_last_message(MsgRole::Claude, "response");
    app.update_last_message(MsgRole::System, "permission needed");
    assert_eq!(app.messages.len(), 2);
    // Resume Claude output
    app.update_last_message(MsgRole::Claude, " continued");
    assert_eq!(app.messages.len(), 3); // New Claude message after system
}
