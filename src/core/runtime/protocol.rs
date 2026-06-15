use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Messages sent TO Claude Code via stdin
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum InputMessage {
    #[serde(rename = "user")]
    User { message: UserMessage },
    #[serde(rename = "control_response")]
    #[allow(dead_code)]
    ControlResponse { response: ControlResponseBody },
    /// `/stop`: cancel the in-progress turn so the conversation can continue.
    ///
    /// Must be a `control_request` with `subtype: "interrupt"`. A bare `{"type":"interrupt"}`
    /// is a no-op in headless stream-json mode (Claude never acks it and keeps generating),
    /// which is why `/stop` must use this control frame (verified against claude 2.1.x).
    #[serde(rename = "control_request")]
    ControlRequest {
        #[serde(rename = "request_id")]
        request_id: String,
        request: ControlRequestInput,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct ControlRequestInput {
    pub subtype: String,
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
    Assistant { message: AssistantMessage },
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
    Error { error: String },
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
    ToolUse { name: String, input: Value },
    #[serde(rename = "tool_result")]
    ToolResult {
        content: Option<String>,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(rename = "image")]
    Image { source: ImageSource },
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

/// Build a `control_request` that cancels the in-progress turn (`/stop`).
pub fn build_interrupt_request(request_id: &str) -> InputMessage {
    InputMessage::ControlRequest {
        request_id: request_id.to_string(),
        request: ControlRequestInput {
            subtype: "interrupt".to_string(),
        },
    }
}

pub fn build_permission_allow(request_id: &str, updated_input: Option<Value>) -> InputMessage {
    InputMessage::ControlResponse {
        response: ControlResponseBody {
            subtype: "success".to_string(),
            request_id: request_id.to_string(),
            response: PermissionResult {
                behavior: "allow".to_string(),
                message: None,
                updated_input,
            },
        },
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// `/stop` must serialize to the `control_request` interrupt that real Claude accepts and
    /// acknowledges (so the turn is cancelled and the session keeps responding). A bare
    /// `{"type":"interrupt"}` does not cancel the turn — see [`InputMessage::ControlRequest`].
    #[test]
    fn interrupt_request_serializes_as_control_request() {
        let msg = build_interrupt_request("stop-123");
        let v: Value = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["type"], "control_request");
        assert_eq!(v["request_id"], "stop-123");
        assert_eq!(v["request"]["subtype"], "interrupt");
    }
}
