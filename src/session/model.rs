use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

// Re-export new types
#[allow(unused_imports)]
pub use crate::session::channel_model::{
    ChannelSession, ClaudeSession, ClaudeSessionState, SessionSource,
};

// Legacy Session struct — kept for backward compatibility during migration.
// TODO: Remove after all callers migrate to ChannelSession + ClaudeSession.
#[derive(Clone, Debug, Serialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub source: SessionSource,
    pub platform: String,
    pub chat_id: String,
    pub work_dir: String,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
}

impl Session {
    pub fn new_webui(title: impl Into<String>, work_dir: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.into(),
            source: SessionSource::WebUI,
            platform: "webui".to_string(),
            chat_id: Uuid::new_v4().to_string(),
            work_dir: work_dir.into(),
            active: true,
            created_at: Utc::now(),
            claude_session_id: None,
        }
    }

    pub fn new_platform(platform: &str, chat_id: impl Into<String>, work_dir: impl Into<String>) -> Self {
        let chat_id_str = chat_id.into();
        Self {
            id: Uuid::new_v4().to_string(),
            title: format!("{} {}", platform, chat_id_str),
            source: match platform {
                "feishu" => SessionSource::Feishu,
                "telegram" => SessionSource::Telegram,
                _ => SessionSource::WebUI,
            },
            platform: platform.to_string(),
            chat_id: chat_id_str,
            work_dir: work_dir.into(),
            active: true,
            created_at: Utc::now(),
            claude_session_id: None,
        }
    }
}
