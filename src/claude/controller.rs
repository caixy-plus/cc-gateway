use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::claude::protocol::{
    build_permission_allow, build_permission_deny, build_user_message, InputMessage, OutputEvent,
};
use crate::claude::session::ClaudeSession;
use crate::config::model::ClaudeConfig;

#[derive(Debug, Clone)]
pub enum ControllerEvent {
    Text(String),
    Thinking(String),
    ToolUse(String, String),
    PermissionRequest(String, String),
    Error(String),
    Done,
}

pub struct ClaudeController {
    config: ClaudeConfig,
    session: Arc<RwLock<Option<ClaudeSession>>>,
    event_tx: mpsc::UnboundedSender<ControllerEvent>,
    event_rx: Arc<Mutex<mpsc::UnboundedReceiver<ControllerEvent>>>,
    work_dir: Arc<RwLock<String>>,
}

impl ClaudeController {
    pub fn new(config: ClaudeConfig) -> Self {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        Self {
            config,
            session: Arc::new(RwLock::new(None)),
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            work_dir: Arc::new(RwLock::new(String::new())),
        }
    }

    pub async fn start_session(&self, work_dir: String) -> Result<()> {
        // Stop existing session if any
        self.stop_session().await?;

        let (claude_tx, mut claude_rx) = mpsc::unbounded_channel::<OutputEvent>();
        let session = ClaudeSession::spawn(work_dir.clone(), &self.config, claude_tx).await?;

        {
            let mut s = self.session.write().await;
            *s = Some(session);
        }
        {
            let mut wd = self.work_dir.write().await;
            *wd = work_dir;
        }

        // Spawn event processor
        let event_tx = self.event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = claude_rx.recv().await {
                Self::process_claude_event(&event_tx, event);
            }
            let _ = event_tx.send(ControllerEvent::Done);
        });

        info!("Claude session started in {}", work_dir);
        Ok(())
    }

    pub async fn stop_session(&self) -> Result<()> {
        let mut s = self.session.write().await;
        if let Some(session) = s.take() {
            session.stop().await?;
            info!("Claude session stopped");
        }
        Ok(())
    }

    pub async fn send_message(&self, text: &str) -> Result<()> {
        let msg = build_user_message(text);
        let mut s = self.session.write().await;
        if let Some(ref mut session) = *s {
            session.send(msg).await?;
            Ok(())
        } else {
            anyhow::bail!("No active Claude session. Use /claude to start one.")
        }
    }

    pub async fn approve_permission(&self, request_id: &str) -> Result<()> {
        let msg = build_permission_allow(request_id);
        let mut s = self.session.write().await;
        if let Some(ref mut session) = *s {
            session.send(msg).await?;
            Ok(())
        } else {
            anyhow::bail!("No active Claude session")
        }
    }

    pub async fn deny_permission(&self, request_id: &str, reason: &str) -> Result<()> {
        let msg = build_permission_deny(request_id, reason);
        let mut s = self.session.write().await;
        if let Some(ref mut session) = *s {
            session.send(msg).await?;
            Ok(())
        } else {
            anyhow::bail!("No active Claude session")
        }
    }

    pub async fn is_session_active(&self) -> bool {
        let s = self.session.read().await;
        s.is_some()
    }

    pub async fn get_work_dir(&self) -> String {
        let wd = self.work_dir.read().await;
        wd.clone()
    }

    pub async fn set_work_dir(&self, dir: String) -> Result<()> {
        {
            let mut wd = self.work_dir.write().await;
            *wd = dir.clone();
        }
        // Restart session with new work dir
        self.start_session(dir).await?;
        Ok(())
    }

    pub async fn set_model(&mut self, model: String) -> Result<()> {
        self.config.model = model;
        let wd = self.get_work_dir().await;
        if !wd.is_empty() {
            self.start_session(wd).await?;
        }
        Ok(())
    }

    pub fn subscribe_events(&self) -> mpsc::UnboundedReceiver<ControllerEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        // Forward existing receiver events to new subscriber
        // For simplicity, we return a new channel and the engine will broadcast
        rx
    }

    pub async fn recv_event(&self) -> Option<ControllerEvent> {
        let mut rx = self.event_rx.lock().await;
        rx.recv().await
    }

    fn process_claude_event(
        event_tx: &mpsc::UnboundedSender<ControllerEvent>,
        event: OutputEvent,
    ) {
        match event {
            OutputEvent::System { session_id } => {
                if let Some(id) = session_id {
                    debug!("Claude session ID: {}", id);
                }
            }
            OutputEvent::Assistant { .. } => {
                if let Some(text) = event.extract_text() {
                    let _ = event_tx.send(ControllerEvent::Text(text));
                }
                if let Some(thinking) = event.extract_thinking() {
                    let _ = event_tx.send(ControllerEvent::Thinking(thinking));
                }
                if let Some((name, input)) = event.extract_tool_use() {
                    let input_str = serde_json::to_string(&input).unwrap_or_default();
                    let _ = event_tx.send(ControllerEvent::ToolUse(name, input_str));
                }
            }
            OutputEvent::Result { result, usage } => {
                if let Some(text) = result {
                    let _ = event_tx.send(ControllerEvent::Text(text));
                }
                if let Some(u) = usage {
                    debug!(
                        "Usage: input={} output={}",
                        u.input_tokens.unwrap_or(0),
                        u.output_tokens.unwrap_or(0)
                    );
                }
                let _ = event_tx.send(ControllerEvent::Done);
            }
            OutputEvent::ControlRequest { .. } => {
                if let Some((req_id, tool_name, _)) = event.is_permission_request() {
                    let _ = event_tx
                        .send(ControllerEvent::PermissionRequest(req_id, tool_name));
                }
            }
            OutputEvent::Error { error } => {
                let _ = event_tx.send(ControllerEvent::Error(error));
            }
            OutputEvent::User { .. } => {
                // Ignore user echo events
            }
        }
    }
}
