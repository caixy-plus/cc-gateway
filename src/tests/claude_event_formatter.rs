use crate::claude::controller::ControllerEvent;
use crate::claude::event_formatter::{
    format_error, format_permission_request, format_thinking, format_tool_result, format_tool_use,
    EventAccumulator,
};

#[test]
fn test_format_thinking() {
    let result = format_thinking("step 1\nstep 2");
    assert!(result.contains("Thinking"));
    assert!(result.contains("step 1"));
}

#[test]
fn test_format_tool_use() {
    let result = format_tool_use("Bash", "{\"cmd\":\"ls\"}");
    assert!(result.contains("Bash"));
    assert!(result.contains("{"));
}

#[test]
fn test_format_tool_result_success() {
    let result = format_tool_result("file1.txt\nfile2.txt", false);
    assert!(result.contains("Tool Result"));
    assert!(result.contains("file1.txt"));
}

#[test]
fn test_format_tool_result_error() {
    let result = format_tool_result("not found", true);
    assert!(result.contains("Tool Error"));
    assert!(result.contains("not found"));
}

#[test]
fn test_format_tool_result_empty() {
    let result = format_tool_result("", false);
    assert!(result.is_empty());
}

#[test]
fn test_format_permission_request() {
    let result = format_permission_request("req-1", "Bash");
    assert!(result.contains("Permission Required"));
    assert!(result.contains("Bash"));
    assert!(result.contains("req-1"));
}

#[test]
fn test_format_error() {
    let result = format_error("something broke");
    assert!(result.contains("Error"));
    assert!(result.contains("something broke"));
}

#[test]
fn test_accumulator_text_only() {
    let mut acc = EventAccumulator::new();
    acc.process_event(&ControllerEvent::Text("Hello ".to_string()));
    acc.process_event(&ControllerEvent::Text("world".to_string()));
    assert!(acc.process_event(&ControllerEvent::Done));
    let output = acc.take_output();
    assert!(output.contains("Hello world"));
}

#[test]
fn test_accumulator_mixed_events() {
    let mut acc = EventAccumulator::new();
    acc.process_event(&ControllerEvent::Text("Hello".to_string()));
    acc.process_event(&ControllerEvent::ToolUse(
        "Bash".to_string(),
        "{\"cmd\":\"ls\"}".to_string(),
    ));
    acc.process_event(&ControllerEvent::ToolResult("file.txt".to_string(), false));
    assert!(acc.process_event(&ControllerEvent::Done));
    let output = acc.take_output();
    assert!(output.contains("Hello"));
    assert!(output.contains("Bash"));
    assert!(output.contains("file.txt"));
}

#[test]
fn test_accumulator_peek_does_not_clear() {
    let mut acc = EventAccumulator::new();
    acc.process_event(&ControllerEvent::Text("test".to_string()));
    let peeked = acc.peek_output();
    assert!(peeked.contains("test"));
    let taken = acc.take_output();
    assert!(taken.contains("test"));
}
