use serde_json::Value;

#[derive(Debug, Clone)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct QuestionItem {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[allow(dead_code)]
    pub multi_select: bool,
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    SessionId(String),
    Text(String),
    Thinking(String),
    ToolUse(String, String),
    ToolResult(String, bool),
    PermissionRequest {
        request_id: String,
        tool_name: String,
        input: Option<Value>,
    },
    ConfirmRequest {
        request_id: String,
        prompt: String,
        options: Vec<String>,
    },
    SelectRequest {
        request_id: String,
        prompt: String,
        options: Vec<String>,
    },
    QuestionRequest {
        request_id: String,
        questions: Vec<QuestionItem>,
    },
    Error(String),
    Done,
}
