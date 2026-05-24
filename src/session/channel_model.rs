use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub struct ChannelSession {
    pub id: String,
    pub title: String,
    pub source: SessionSource,
    pub platform: String,
    pub channel_id: String,
    pub work_dir: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ClaudeSession {
    pub id: String,
    pub channel_session_id: String,
    pub title: String,
    pub work_dir: String,
    pub active: bool,
    pub state: ClaudeSessionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum SessionSource {
    WebUI,
    Feishu,
    Telegram,
    TUI,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub enum ClaudeSessionState {
    Active,
    Stopped,
    Dead,
}

impl std::fmt::Display for ClaudeSessionState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaudeSessionState::Active => write!(f, "active"),
            ClaudeSessionState::Stopped => write!(f, "stopped"),
            ClaudeSessionState::Dead => write!(f, "dead"),
        }
    }
}

impl std::str::FromStr for ClaudeSessionState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "active" => Ok(ClaudeSessionState::Active),
            "stopped" => Ok(ClaudeSessionState::Stopped),
            "dead" => Ok(ClaudeSessionState::Dead),
            _ => Err(format!("Unknown ClaudeSessionState: {}", s)),
        }
    }
}

impl ChannelSession {
    pub fn new_webui(title: impl Into<String>, work_dir: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: title.into(),
            source: SessionSource::WebUI,
            platform: "webui".to_string(),
            channel_id: Uuid::new_v4().to_string(),
            work_dir: work_dir.into(),
            created_at: Utc::now(),
        }
    }

    pub fn new_platform(
        platform: &str,
        channel_id: impl Into<String>,
        work_dir: impl Into<String>,
    ) -> Self {
        let channel_id_str = channel_id.into();
        Self {
            id: Uuid::new_v4().to_string(),
            title: format!("{} {}", platform, channel_id_str),
            source: match platform {
                "feishu" => SessionSource::Feishu,
                "telegram" => SessionSource::Telegram,
                "tui" => SessionSource::TUI,
                _ => SessionSource::WebUI,
            },
            platform: platform.to_string(),
            channel_id: channel_id_str,
            work_dir: work_dir.into(),
            created_at: Utc::now(),
        }
    }

    #[allow(dead_code)]
    pub fn new_tui(work_dir: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            title: "TUI".to_string(),
            source: SessionSource::TUI,
            platform: "tui".to_string(),
            channel_id: "tui".to_string(),
            work_dir: work_dir.into(),
            created_at: Utc::now(),
        }
    }
}

impl ClaudeSession {
    pub fn new(
        channel_session_id: impl Into<String>,
        title: impl Into<String>,
        work_dir: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            channel_session_id: channel_session_id.into(),
            title: title.into(),
            work_dir: work_dir.into(),
            active: false,
            state: ClaudeSessionState::Stopped,
            claude_session_id: None,
            created_at: Utc::now(),
            stopped_at: None,
            updated_at: Some(Utc::now()),
        }
    }
}
