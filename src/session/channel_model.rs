use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The source of a channel session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionSource {
    WebUI,
    Feishu,
    Telegram,
    TUI,
}

impl fmt::Display for SessionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionSource::WebUI => write!(f, "WebUI"),
            SessionSource::Feishu => write!(f, "Feishu"),
            SessionSource::Telegram => write!(f, "Telegram"),
            SessionSource::TUI => write!(f, "TUI"),
        }
    }
}

/// Persistent channel session — represents a communication channel
/// (chat window, WebUI tab, TUI session). Each channel can have
/// multiple Claude sessions over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSession {
    pub id: String,
    pub title: String,
    pub source: SessionSource,
    pub platform: String,
    pub channel_id: String,
    pub work_dir: String,
    pub created_at: DateTime<Utc>,
}

/// Lifecycle state of a Claude session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaudeSessionState {
    Stopped,
    Starting,
    Active,
}

impl ClaudeSessionState {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "stopped" => Ok(ClaudeSessionState::Stopped),
            "starting" => Ok(ClaudeSessionState::Starting),
            "active" => Ok(ClaudeSessionState::Active),
            other => Err(format!("Unknown ClaudeSessionState: {}", other)),
        }
    }
}

impl fmt::Display for ClaudeSessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClaudeSessionState::Stopped => write!(f, "stopped"),
            ClaudeSessionState::Starting => write!(f, "starting"),
            ClaudeSessionState::Active => write!(f, "active"),
        }
    }
}

impl std::str::FromStr for ClaudeSessionState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Persistent Claude session — represents a single Claude Code process
/// instance within a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeSession {
    pub id: String,
    pub channel_session_id: String,
    pub provider: String,
    pub title: String,
    pub work_dir: String,
    pub active: bool,
    pub state: ClaudeSessionState,
    pub provider_session_id: Option<String>,
    pub claude_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl ClaudeSession {
    pub fn new(channel_session_id: &str, title: &str, work_dir: &str) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            channel_session_id: channel_session_id.to_string(),
            provider: "claude".to_string(),
            title: title.to_string(),
            work_dir: work_dir.to_string(),
            active: false,
            state: ClaudeSessionState::Stopped,
            provider_session_id: None,
            claude_session_id: None,
            created_at: now,
            stopped_at: None,
            updated_at: None,
        }
    }
}
