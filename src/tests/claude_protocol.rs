use crate::runtime::protocol::{
    build_permission_allow, build_permission_deny, build_user_message, ContentBlock, OutputEvent,
};
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
    let msg = build_permission_allow("req-1", None);
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
        message: crate::runtime::protocol::AssistantMessage {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Text {
                    text: "Hello ".to_string(),
                },
                ContentBlock::Text {
                    text: "world".to_string(),
                },
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
        message: crate::runtime::protocol::AssistantMessage {
            role: "assistant".to_string(),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "think1".to_string(),
                },
                ContentBlock::Thinking {
                    thinking: "think2".to_string(),
                },
            ],
        },
    };
    assert_eq!(event.extract_thinking(), Some("think1\nthink2".to_string()));
}

#[test]
fn test_extract_tool_use() {
    let event = OutputEvent::Assistant {
        message: crate::runtime::protocol::AssistantMessage {
            role: "assistant".to_string(),
            content: vec![ContentBlock::ToolUse {
                name: "Bash".to_string(),
                input: json!({"cmd":"ls"}),
            }],
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
        request: crate::runtime::protocol::ControlRequestBody {
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
        request: crate::runtime::protocol::ControlRequestBody {
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
        request: crate::runtime::protocol::ControlRequestBody {
            subtype: "select_option".to_string(),
            tool_name: None,
            input: Some(json!({"options": ["A", "B", "C"], "prompt": "Choose one"})),
        },
    };
    let (req_id, label, input) = event.is_permission_request().unwrap();
    assert_eq!(req_id, "req-select");
    assert_eq!(label, "select_option");
    assert_eq!(
        input,
        Some(json!({"options": ["A", "B", "C"], "prompt": "Choose one"}))
    );
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
