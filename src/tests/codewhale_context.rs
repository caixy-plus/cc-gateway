use crate::agent::codewhale_context::{
    build_prompt, compress_turns_to_budget, parse_capability_json, ContextPolicy, Turn,
};

#[test]
fn doctor_capability_parses_context_window() {
    let json = r#"{"capability":{"resolved_model":"deepseek-v4-flash","context_window":1000000,"max_output":384000}}"#;
    let cap = parse_capability_json(json).expect("cap");
    assert_eq!(cap.context_window, 1_000_000);
}

#[test]
fn prompt_template_separates_history_and_current_user() {
    let prompt = build_prompt("/tmp/w", Some("User: one\n\nAssistant: two"), "three");
    assert!(prompt.contains("[Conversation history]"));
    assert!(prompt.contains("User: three"));
}

#[test]
fn truncation_prefers_recent_turns() {
    let turns: Vec<Turn> = (0..10)
        .map(|i| Turn {
            user: format!("u{i}"),
            assistant: Some(format!("long assistant answer number {i} with extra words")),
        })
        .collect();
    let policy = ContextPolicy {
        keep_recent_turns: 2,
        min_recent_turns: 1,
        pin_first_user: false,
        max_message_chars: 80,
        ..Default::default()
    };
    let out = compress_turns_to_budget(turns, 80, &policy);
    assert!(out.contains("u9"));
    assert!(!out.contains("u0"));
}
