use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The source of a channel session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionSource {
    WebUI,
    Feishu,
    Telegram,
}

impl fmt::Display for SessionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionSource::WebUI => write!(f, "WebUI"),
            SessionSource::Feishu => write!(f, "Feishu"),
            SessionSource::Telegram => write!(f, "Telegram"),
        }
    }
}

/// Persistent channel session — represents a communication channel
/// (chat window, WebUI tab, bot chat). Each channel can have
/// multiple agent sessions over time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSession {
    pub id: String,
    pub title: String,
    pub source: SessionSource,
    pub platform: String,
    pub channel_id: String,
    pub work_dir: String,
    /// Channel-level default agent for `/agent` when no provider prefix is given.
    pub default_provider: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Lifecycle state of a persisted agent session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentSessionState {
    Stopped,
    Starting,
    Active,
}

impl AgentSessionState {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "stopped" => Ok(AgentSessionState::Stopped),
            "starting" => Ok(AgentSessionState::Starting),
            "active" => Ok(AgentSessionState::Active),
            other => Err(format!("Unknown AgentSessionState: {}", other)),
        }
    }
}

impl fmt::Display for AgentSessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentSessionState::Stopped => write!(f, "stopped"),
            AgentSessionState::Starting => write!(f, "starting"),
            AgentSessionState::Active => write!(f, "active"),
        }
    }
}

impl std::str::FromStr for AgentSessionState {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Persistent agent session — one provider subprocess instance within a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: String,
    pub channel_session_id: String,
    pub provider: String,
    pub title: String,
    pub work_dir: String,
    pub active: bool,
    pub state: AgentSessionState,
    pub provider_session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl AgentSession {
    pub fn stored_provider(&self) -> crate::config::model::AgentProvider {
        crate::config::model::AgentProvider::parse_str(&self.provider)
    }

    pub fn resume_provider_session_id(&self) -> Option<String> {
        self.provider_session_id.clone()
    }

    pub fn display_session_id(&self) -> &str {
        self.provider_session_id.as_deref().unwrap_or(&self.id)
    }

    #[cfg(test)]
    pub fn new(channel_session_id: &str, title: &str, work_dir: &str) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            channel_session_id: channel_session_id.to_string(),
            provider: "claude".to_string(),
            title: title.to_string(),
            work_dir: work_dir.to_string(),
            active: false,
            state: AgentSessionState::Stopped,
            provider_session_id: None,
            created_at: now,
            stopped_at: None,
            updated_at: None,
        }
    }
}
