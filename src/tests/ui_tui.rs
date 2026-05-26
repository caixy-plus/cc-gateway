use crate::cli::tui::{should_handle_key_event, ChatMessage, MsgRole};
use crate::command::builtin::{
    interactive_select_with_backend, render_file_list, SelectAction, SelectBackend,
};

#[test]
fn appending_claude_chunks_preserves_new_lines() {
    let mut message = ChatMessage::new(MsgRole::Claude, "first");

    message.append("\nsecond\nthird");

    assert_eq!(message.lines, vec!["first", "second", "third"]);
}

#[test]
fn render_file_list_keeps_selected_item_visible() {
    let items: Vec<(String, bool)> = (0..8).map(|idx| (format!("dir-{idx}"), true)).collect();

    let (lines, scroll_row) = render_file_list(&items, 80, 5, 6);

    assert_eq!(scroll_row, 5);
    assert_eq!(lines, vec!["dir-5/", ">dir-6/"]);
}

#[test]
fn tui_ignores_key_release_events() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    let press = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Press);
    let repeat = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Repeat);
    let release = KeyEvent::new_with_kind(KeyCode::Down, KeyModifiers::NONE, KeyEventKind::Release);

    assert!(should_handle_key_event(&press));
    assert!(should_handle_key_event(&repeat));
    assert!(!should_handle_key_event(&release));
}

struct TestBackend {
    keys: Vec<crossterm::event::KeyCode>,
    frames: Vec<Vec<String>>,
}

impl SelectBackend for TestBackend {
    fn size(&self) -> (u16, u16) {
        (80, 10)
    }

    fn draw(&mut self, lines: &[String]) {
        self.frames.push(lines.to_vec());
    }

    fn read_key(&mut self) -> Option<crossterm::event::KeyCode> {
        if self.keys.is_empty() {
            None
        } else {
            Some(self.keys.remove(0))
        }
    }
}

#[test]
fn interactive_select_backend_selects_directory() {
    let items = vec![
        ("a".to_string(), true),
        ("b".to_string(), true),
        ("c".to_string(), true),
    ];
    let mut backend = TestBackend {
        keys: vec![
            crossterm::event::KeyCode::Down,
            crossterm::event::KeyCode::Enter,
        ],
        frames: Vec::new(),
    };

    let selected = interactive_select_with_backend(&items, &mut backend);

    assert!(matches!(selected, SelectAction::Selected(1)));
    assert!(!backend.frames.is_empty());
}

#[test]
fn interactive_select_backend_cancels_when_input_ends() {
    let items = vec![("a".to_string(), true)];
    let mut backend = TestBackend {
        keys: Vec::new(),
        frames: Vec::new(),
    };

    let selected = interactive_select_with_backend(&items, &mut backend);

    assert!(matches!(selected, SelectAction::Cancelled));
    assert!(!backend.frames.is_empty());
}
