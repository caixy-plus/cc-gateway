//! Structured WebUI chat interactions (model picker, etc.) via SSE/history `content` prefixes.

use crate::command::agents;
use crate::command::models;
use crate::config::model::AgentProvider;

/// Prefix for model-picker payloads in SSE / history `content`.
pub const WEBUI_MODEL_PICKER_PREFIX: &str = "__ccg_models__:";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebUiModelPickerPayload {
    pub v: u8,
    pub kind: String,
    pub provider: String,
    pub provider_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    pub options: Vec<String>,
}

pub fn build_model_picker_content(
    provider: &AgentProvider,
    current: Option<&str>,
    options: &[String],
) -> String {
    let payload = WebUiModelPickerPayload {
        v: 1,
        kind: "model_picker".to_string(),
        provider: provider.to_string(),
        provider_name: agents::provider_display_name(provider).to_string(),
        current: current.map(str::to_string),
        options: options.to_vec(),
    };
    let json = serde_json::to_string(&payload).expect("model picker json");
    format!("{WEBUI_MODEL_PICKER_PREFIX}{json}")
}

pub fn model_picker_title_line(provider: &AgentProvider, current: Option<&str>) -> String {
    let mut lines = vec![crate::t_fmt!(
        "models.title",
        NAME = agents::provider_display_name(provider)
    )];
    lines.push(models::current_model_line(current));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::AgentProvider;

    #[test]
    fn model_picker_content_has_prefix_and_options() {
        let s = build_model_picker_content(
            &AgentProvider::Pi,
            Some("anthropic/claude"),
            &["a".to_string(), "b".to_string()],
        );
        assert!(s.starts_with(WEBUI_MODEL_PICKER_PREFIX));
        assert!(s.contains("model_picker"));
        assert!(s.contains("anthropic/claude"));
        assert!(s.contains("\"a\""));
    }
}
