use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Messages sent TO Claude Code via stdin
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum InputMessage {
    #[serde(rename = "user")]
    User {
        message: UserMessage,
    },
    #[serde(rename = "control_response")]
    #[allow(dead_code)]
    ControlResponse {
        response: ControlResponseBody,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct UserMessage {
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlResponseBody {
    pub subtype: String,
    #[serde(rename = "request_id")]
    pub request_id: String,
    pub response: PermissionResult,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionResult {
    pub behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(rename = "updatedInput", skip_serializing_if = "Option::is_none")]
    pub updated_input: Option<Value>,
}

/// Events received FROM Claude Code via stdout
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum OutputEvent {
    #[serde(rename = "system")]
    System {
        #[serde(rename = "session_id")]
        session_id: Option<String>,
    },
    #[serde(rename = "assistant")]
    Assistant {
        message: AssistantMessage,
    },
    #[serde(rename = "user")]
    User {
        #[allow(dead_code)]
        message: UserEventMessage,
    },
    #[serde(rename = "result")]
    Result {
        result: Option<String>,
        usage: Option<UsageInfo>,
    },
    #[serde(rename = "control_request")]
    ControlRequest {
        #[serde(rename = "request_id")]
        request_id: String,
        request: ControlRequestBody,
    },
    #[serde(rename = "error")]
    Error {
        error: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct AssistantMessage {
    #[allow(dead_code)]
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserEventMessage {
    #[allow(dead_code)]
    pub role: String,
    #[allow(dead_code)]
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        content: Option<String>,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(rename = "image")]
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(rename = "media_type")]
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ControlRequestBody {
    pub subtype: String,
    #[serde(rename = "tool_name")]
    pub tool_name: Option<String>,
    pub input: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsageInfo {
    #[serde(rename = "input_tokens")]
    pub input_tokens: Option<u32>,
    #[serde(rename = "output_tokens")]
    pub output_tokens: Option<u32>,
}

impl OutputEvent {
    #[allow(dead_code)]
    pub fn extract_text(&self) -> Option<String> {
        match self {
            OutputEvent::Assistant { message } => {
                let mut texts = Vec::new();
                for block in &message.content {
                    if let ContentBlock::Text { text } = block {
                        texts.push(text.clone());
                    }
                }
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join(""))
                }
            }
            OutputEvent::Result { result, .. } => result.clone(),
            OutputEvent::Error { error } => Some(format!("Error: {}", error)),
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn extract_thinking(&self) -> Option<String> {
        match self {
            OutputEvent::Assistant { message } => {
                let mut thoughts = Vec::new();
                for block in &message.content {
                    if let ContentBlock::Thinking { thinking } = block {
                        thoughts.push(thinking.clone());
                    }
                }
                if thoughts.is_empty() {
                    None
                } else {
                    Some(thoughts.join("\n"))
                }
            }
            _ => None,
        }
    }

    #[allow(dead_code)]
    pub fn extract_tool_use(&self) -> Option<(String, Value)> {
        match self {
            OutputEvent::Assistant { message } => {
                for block in &message.content {
                    if let ContentBlock::ToolUse { name, input } = block {
                        return Some((name.clone(), input.clone()));
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn is_permission_request(&self) -> Option<(String, String, Option<Value>)> {
        match self {
            OutputEvent::ControlRequest {
                request_id,
                request,
            } => {
                let label = request
                    .tool_name
                    .clone()
                    .unwrap_or_else(|| request.subtype.clone());
                Some((request_id.clone(), label, request.input.clone()))
            }
            _ => None,
        }
    }

    pub fn extract_control_subtype(&self) -> Option<String> {
        match self {
            OutputEvent::ControlRequest { request, .. } => Some(request.subtype.clone()),
            _ => None,
        }
    }
}

pub fn build_user_message(text: &str) -> InputMessage {
    InputMessage::User {
        message: UserMessage {
            role: "user".to_string(),
            content: Value::String(text.to_string()),
        },
    }
}

#[allow(dead_code)]
pub fn build_permission_allow(request_id: &str) -> InputMessage {
    InputMessage::ControlResponse {
        response: ControlResponseBody {
            subtype: "success".to_string(),
            request_id: request_id.to_string(),
            response: PermissionResult {
                behavior: "allow".to_string(),
                message: None,
                updated_input: None,
            },
        },
    }
}

#[allow(dead_code)]
pub fn build_permission_deny(request_id: &str, message: &str) -> InputMessage {
    InputMessage::ControlResponse {
        response: ControlResponseBody {
            subtype: "success".to_string(),
            request_id: request_id.to_string(),
            response: PermissionResult {
                behavior: "deny".to_string(),
                message: Some(message.to_string()),
                updated_input: None,
            },
        },
    }
}

/// Build a user message containing a tool_result content block.
/// This is used to feed MCP-executed tool results back into the Claude conversation.
#[allow(dead_code)]
pub fn build_tool_result_user_message(content: &str, is_error: bool) -> InputMessage {
    let content_blocks = json!([
        {
            "type": "tool_result",
            "content": content,
            "is_error": is_error
        }
    ]);
    InputMessage::User {
        message: UserMessage {
            role: "user".to_string(),
            content: content_blocks,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_build_user_message() {
        let msg = build_user_message("hello");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"user\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hello\""));
    }

    #[test]
    fn test_build_permission_allow() {
        let msg = build_permission_allow("req-1");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"control_response\""));
        assert!(json.contains("\"subtype\":\"success\""));
        assert!(json.contains("\"request_id\":\"req-1\""));
        assert!(json.contains("\"behavior\":\"allow\""));
    }

    #[test]
    fn test_build_permission_deny() {
        let msg = build_permission_deny("req-1", "nope");
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"behavior\":\"deny\""));
        assert!(json.contains("\"message\":\"nope\""));
    }

    #[test]
    fn test_extract_text_from_assistant() {
        let event = OutputEvent::Assistant {
            message: AssistantMessage {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Text { text: "Hello ".to_string() },
                    ContentBlock::Text { text: "world".to_string() },
                ],
            },
        };
        assert_eq!(event.extract_text(), Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_from_result() {
        let event = OutputEvent::Result {
            result: Some("done".to_string()),
            usage: None,
        };
        assert_eq!(event.extract_text(), Some("done".to_string()));
    }

    #[test]
    fn test_extract_text_from_error() {
        let event = OutputEvent::Error {
            error: "oops".to_string(),
        };
        assert_eq!(event.extract_text(), Some("Error: oops".to_string()));
    }

    #[test]
    fn test_extract_thinking() {
        let event = OutputEvent::Assistant {
            message: AssistantMessage {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::Thinking { thinking: "think1".to_string() },
                    ContentBlock::Thinking { thinking: "think2".to_string() },
                ],
            },
        };
        assert_eq!(event.extract_thinking(), Some("think1\nthink2".to_string()));
    }

    #[test]
    fn test_extract_tool_use() {
        let event = OutputEvent::Assistant {
            message: AssistantMessage {
                role: "assistant".to_string(),
                content: vec![
                    ContentBlock::ToolUse { name: "Bash".to_string(), input: json!({"cmd":"ls"}) },
                ],
            },
        };
        let (name, input) = event.extract_tool_use().unwrap();
        assert_eq!(name, "Bash");
        assert_eq!(input, json!({"cmd":"ls"}));
    }

    #[test]
    fn test_is_permission_request_tool() {
        let event = OutputEvent::ControlRequest {
            request_id: "req-1".to_string(),
            request: ControlRequestBody {
                subtype: "can_use_tool".to_string(),
                tool_name: Some("Bash".to_string()),
                input: Some(json!({"command":"ls"})),
            },
        };
        let (req_id, label, input) = event.is_permission_request().unwrap();
        assert_eq!(req_id, "req-1");
        assert_eq!(label, "Bash");
        assert_eq!(input, Some(json!({"command":"ls"})));
    }

    #[test]
    fn test_is_permission_request_non_tool_subtype() {
        let event = OutputEvent::ControlRequest {
            request_id: "req-1".to_string(),
            request: ControlRequestBody {
                subtype: "confirm".to_string(),
                tool_name: None,
                input: Some(json!({"options": ["yes", "no"]})),
            },
        };
        let (req_id, label, input) = event.is_permission_request().unwrap();
        assert_eq!(req_id, "req-1");
        assert_eq!(label, "confirm");
        assert_eq!(input, Some(json!({"options": ["yes", "no"]})));
    }

    #[test]
    fn test_is_permission_request_select_options() {
        let event = OutputEvent::ControlRequest {
            request_id: "req-select".to_string(),
            request: ControlRequestBody {
                subtype: "select_option".to_string(),
                tool_name: None,
                input: Some(json!({"options": ["A", "B", "C"], "prompt": "Choose one"})),
            },
        };
        let (req_id, label, input) = event.is_permission_request().unwrap();
        assert_eq!(req_id, "req-select");
        assert_eq!(label, "select_option");
        assert_eq!(input, Some(json!({"options": ["A", "B", "C"], "prompt": "Choose one"})));
    }

    #[test]
    fn test_is_permission_request_not_control() {
        let event = OutputEvent::System {
            session_id: Some("sid".to_string()),
        };
        assert!(event.is_permission_request().is_none());
    }

    #[test]
    fn test_deserialize_output_events() {
        let system_json = r#"{"type":"system","session_id":"abc-123"}"#;
        let event: OutputEvent = serde_json::from_str(system_json).unwrap();
        match event {
            OutputEvent::System { session_id } => assert_eq!(session_id, Some("abc-123".to_string())),
            _ => panic!("Expected System event"),
        }

        let assistant_json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#;
        let event: OutputEvent = serde_json::from_str(assistant_json).unwrap();
        assert_eq!(event.extract_text(), Some("hi".to_string()));
    }
}
