#![allow(dead_code)]
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde_json::Value;

use crate::runtime::protocol::{ControlResponseBody, InputMessage, PermissionResult};

/// Type of interactive request from Claude Code.
#[derive(Debug, Clone)]
pub enum InteractionType {
    /// Tool permission request.
    Permission {
        tool_name: String,
        input: Option<Value>,
    },
    /// Single-select question.
    SingleSelect {
        prompt: String,
        options: Vec<String>,
    },
    /// Multi-select question.
    MultiSelect {
        prompt: String,
        options: Vec<String>,
        selected: Vec<String>,
    },
    /// Free-text input question.
    TextInput { prompt: String },
    /// Yes/No confirmation.
    Confirm { prompt: String },
    /// Structured multi-question form (AskUserQuestion).
    Question {
        questions: Vec<QuestionDef>,
        answers: serde_json::Map<String, Value>,
    },
}

/// Definition of a single question inside an AskUserQuestion form.
#[derive(Debug, Clone)]
pub struct QuestionDef {
    pub question: String,
    pub header: String,
    pub options: Vec<String>,
    pub multi_select: bool,
}

/// Lifecycle state of a pending interaction.
#[derive(Debug, Clone)]
pub enum InteractionState {
    /// Waiting for user response.
    Waiting,
    /// Intermediate state for multi-select (partial selections).
    Partial(Value),
    /// Final answer has been collected.
    Answered(Value),
}

/// A pending interaction stored while waiting for the user to respond via Feishu card.
#[derive(Debug, Clone)]
pub struct PendingInteraction {
    pub request_id: String,
    pub interaction_type: InteractionType,
    pub state: InteractionState,
    pub chat_id: String,
    pub sender_open_id: String,
    /// Feishu card message ID (used to update the card after answer).
    pub message_id: String,
    pub created_at: Instant,
}

/// In-memory store for pending interactions with TTL-based expiration.
pub struct InteractionStore {
    inner: DashMap<String, PendingInteraction>,
    ttl: Duration,
}

impl InteractionStore {
    /// Create a new store with a default TTL of 10 minutes.
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
            ttl: Duration::from_secs(600),
        }
    }

    /// Insert a new pending interaction.
    pub fn insert(&self, interaction: PendingInteraction) {
        self.inner
            .insert(interaction.request_id.clone(), interaction);
    }

    /// Get a clone of a pending interaction by request_id.
    pub fn get(&self, request_id: &str) -> Option<PendingInteraction> {
        self.inner.get(request_id).map(|entry| entry.clone())
    }

    /// Update the state of an existing interaction.
    /// Returns true if the interaction existed and was updated.
    pub fn update_state(&self, request_id: &str, state: InteractionState) -> bool {
        if let Some(mut entry) = self.inner.get_mut(request_id) {
            entry.state = state;
            true
        } else {
            false
        }
    }

    /// Remove and return a pending interaction by request_id.
    pub fn take(&self, request_id: &str) -> Option<PendingInteraction> {
        self.inner.remove(request_id).map(|(_, v)| v)
    }

    /// Find a pending interaction in Waiting state for a given chat_id.
    pub fn find_waiting_by_chat_id(&self, chat_id: &str) -> Option<PendingInteraction> {
        self.inner
            .iter()
            .find(|entry| {
                let v = entry.value();
                matches!(v.state, InteractionState::Waiting) && v.chat_id == chat_id
            })
            .map(|entry| entry.clone())
    }

    /// Remove all interactions older than the configured TTL.
    pub fn cleanup_expired(&self) {
        let now = Instant::now();
        self.inner
            .retain(|_, v| now.duration_since(v.created_at) < self.ttl);
    }
}

impl Default for InteractionStore {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helper functions to build InputMessage::ControlResponse
// ---------------------------------------------------------------------------

/// Build a response that allows the requested tool.
pub fn build_allow_response(request_id: &str) -> InputMessage {
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

/// Build a response that denies the requested tool.
pub fn build_deny_response(request_id: &str, message: &str) -> InputMessage {
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

/// Build a response for selection-type interactions (single-select, multi-select, confirm).
/// `answer` should be the JSON value representing the user's choice.
pub fn build_select_response(request_id: &str, answer: Value) -> InputMessage {
    InputMessage::ControlResponse {
        response: ControlResponseBody {
            subtype: "success".to_string(),
            request_id: request_id.to_string(),
            response: PermissionResult {
                behavior: "allow".to_string(),
                message: None,
                updated_input: Some(answer),
            },
        },
    }
}

/// Build a response for `AskUserQuestion` interactions.
///
/// `answers` maps each question text to either a single answer string or an array of strings
/// (for multi-select questions). The resulting `updated_input` follows the format:
/// ```json
/// {
///   "questions": [...],
///   "answers": { "question": "answer" }
/// }
/// ```
pub fn build_question_response(
    request_id: &str,
    questions: &[QuestionDef],
    answers: &serde_json::Map<String, Value>,
) -> InputMessage {
    let questions_json: Vec<Value> = questions
        .iter()
        .map(|q| {
            serde_json::json!({
                "question": q.question,
                "header": q.header,
                "options": q.options,
                "multi_select": q.multi_select,
            })
        })
        .collect();

    let updated_input = serde_json::json!({
        "questions": questions_json,
        "answers": answers,
    });

    InputMessage::ControlResponse {
        response: ControlResponseBody {
            subtype: "success".to_string(),
            request_id: request_id.to_string(),
            response: PermissionResult {
                behavior: "allow".to_string(),
                message: None,
                updated_input: Some(updated_input),
            },
        },
    }
}
