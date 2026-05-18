use crate::claude::controller::ControllerEvent;

/// Format a thinking block into a display string.
pub fn format_thinking(thinking: &str) -> String {
    format!("💭 Thinking...\n{}", thinking)
}

/// Format a tool use block into a display string.
pub fn format_tool_use(name: &str, input: &str) -> String {
    let first_line = input.lines().next().unwrap_or("");
    if first_line.is_empty() {
        format!("🔧 Tool: {}", name)
    } else {
        format!("🔧 Tool: {}\n  {}", name, first_line)
    }
}

/// Format a tool result block into a display string.
pub fn format_tool_result(content: &str, is_error: bool) -> String {
    if content.is_empty() {
        return String::new();
    }
    let prefix = if is_error { "❌ Tool Error" } else { "✅ Tool Result" };
    let mut result = format!("{}\n", prefix);
    for line in content.lines() {
        result.push_str(&format!("  {}\n", line));
    }
    result
}

/// Format a permission request into a display string.
pub fn format_permission_request(req_id: &str, tool_name: &str) -> String {
    format!(
        "⚠️  Permission Required\n  Tool: {}\n  Request ID: {}\n  Type /allow or /deny [reason] to respond.",
        tool_name, req_id
    )
}

/// Format an error into a display string.
pub fn format_error(err: &str) -> String {
    format!("❌ Error: {}", err)
}

/// Accumulates controller events into a single formatted message.
/// Used by both CLI and Feishu to ensure consistent formatting logic.
pub struct EventAccumulator {
    text_buffer: String,
    accumulated: String,
    in_progress: bool,
}

impl EventAccumulator {
    pub fn new() -> Self {
        Self {
            text_buffer: String::new(),
            accumulated: String::new(),
            in_progress: false,
        }
    }

    /// Process a single controller event.
    /// Returns `true` if the response is complete (Done event received).
    pub fn process_event(&mut self, event: &ControllerEvent) -> bool {
        match event {
            ControllerEvent::Text(text) => {
                self.text_buffer.push_str(text);
                self.in_progress = true;
                false
            }
            ControllerEvent::Thinking(thinking) => {
                self.flush_text();
                self.accumulated.push_str(&format_thinking(thinking));
                self.accumulated.push('\n');
                false
            }
            ControllerEvent::ToolUse(name, input) => {
                self.flush_text();
                self.accumulated.push_str(&format_tool_use(name, input));
                self.accumulated.push('\n');
                false
            }
            ControllerEvent::ToolResult(content, is_error) => {
                self.flush_text();
                let formatted = format_tool_result(content, *is_error);
                if !formatted.is_empty() {
                    self.accumulated.push_str(&formatted);
                    self.accumulated.push('\n');
                }
                false
            }
            ControllerEvent::PermissionRequest(req_id, tool_name) => {
                self.flush_text();
                self.accumulated.push_str(&format_permission_request(req_id, tool_name));
                self.accumulated.push('\n');
                false
            }
            ControllerEvent::Error(err) => {
                self.flush_text();
                self.accumulated.push_str(&format_error(err));
                self.accumulated.push('\n');
                false
            }
            ControllerEvent::Done => {
                self.flush_text();
                true
            }
        }
    }

    fn flush_text(&mut self) {
        if self.in_progress {
            self.accumulated.push_str(&self.text_buffer);
            self.accumulated.push('\n');
            self.text_buffer.clear();
            self.in_progress = false;
        }
    }

    /// Take the accumulated output, clearing the internal buffer.
    pub fn take_output(&mut self) -> String {
        self.flush_text();
        std::mem::take(&mut self.accumulated)
    }

    /// Peek at the current accumulated output without clearing.
    #[allow(dead_code)]
    pub fn peek_output(&self) -> String {
        let mut result = self.accumulated.clone();
        if self.in_progress {
            result.push_str(&self.text_buffer);
        }
        result
    }

}

impl Default for EventAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        acc.process_event(&ControllerEvent::ToolUse("Bash".to_string(), "{\"cmd\":\"ls\"}".to_string()));
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
}
