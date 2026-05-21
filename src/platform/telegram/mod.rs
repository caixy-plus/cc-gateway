use anyhow::{Context, Result};
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::claude::controller::{ClaudeController, ControllerEvent};
use crate::claude::event_formatter::EventAccumulator;
use crate::command::router::CommandRouter;
use crate::config::model::{ClaudeConfig, TelegramConfig};
use crate::platform::Platform;

/// Per-chat session for Telegram (same pattern as Feishu).
#[derive(Clone)]
struct TgChatSession {
    controller: Arc<Mutex<ClaudeController>>,
    router: Arc<CommandRouter>,
}

impl TgChatSession {
    fn new(claude_config: ClaudeConfig, show_thinking: bool, default_dir: &str) -> Self {
        let controller = Arc::new(Mutex::new(ClaudeController::new(claude_config, show_thinking)));
        let router = Arc::new(CommandRouter::new(controller.clone(), default_dir));
        Self { controller, router }
    }
}

#[derive(Clone)]
pub struct TelegramPlatform {
    config: TelegramConfig,
    default_dir: String,
    claude_config: ClaudeConfig,
    show_thinking: bool,
    http_client: reqwest::Client,
    sessions: Arc<DashMap<String, TgChatSession>>,
    offset: Arc<AtomicI64>,
}

impl TelegramPlatform {
    pub fn new(
        config: TelegramConfig,
        default_dir: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
    ) -> Self {
        Self {
            config,
            default_dir: default_dir.to_string(),
            claude_config,
            show_thinking,
            http_client: reqwest::Client::new(),
            sessions: Arc::new(DashMap::new()),
            offset: Arc::new(AtomicI64::new(0)),
        }
    }

    fn api_url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.config.bot_token, method)
    }

    async fn get_updates(&self) -> Result<Vec<Update>> {
        let url = format!(
            "{}?offset={}&limit=100&timeout=30",
            self.api_url("getUpdates"),
            self.offset.load(Ordering::SeqCst)
        );
        let resp = self.http_client.get(&url).send().await?;
        let body: Value = resp.json().await?;

        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            let desc = body.get("description").and_then(|v| v.as_str()).unwrap_or("unknown");
            anyhow::bail!("Telegram API error: {}", desc);
        }

        let updates: Vec<Update> = serde_json::from_value(
            body.get("result").cloned().unwrap_or(json!([]))
        )?;
        Ok(updates)
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<()> {
        let url = self.api_url("sendMessage");
        let payload = json!({
            "chat_id": chat_id,
            "text": text,
        });

        let resp = self.http_client
            .post(&url)
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await?;
            anyhow::bail!("Telegram sendMessage failed: {}", body);
        }
        Ok(())
    }

    async fn handle_update(&self, update: Update) -> Result<()> {
        let msg = match update.message {
            Some(m) => m,
            None => return Ok(()),
        };

        let chat_id = msg.chat.id;
        let user_id = msg.from.as_ref().map(|u| u.id).unwrap_or(0);
        let username = msg.from.as_ref().and_then(|u| u.username.clone()).unwrap_or_default();

        if !self.is_allowed_sender(user_id, &username) {
            debug!("Telegram message from unauthorized user: {} (@{})", user_id, username);
            return Ok(());
        }

        let content = msg.text.unwrap_or_default();
        if content.is_empty() {
            return Ok(());
        }

        let chat_id_str = chat_id.to_string();
        let session = self.get_session(&chat_id_str).await;

        let session_active = {
            let ctrl = session.controller.lock().await;
            ctrl.is_session_active().await
        };

        if !content.is_empty() && content.starts_with('/') && !session_active {
            let known = ["/help", "/cd", "/cd_default", "/claude", "/ll", "/mkdir", "/quit", "/pwd", "/show-thinking", "/hide-thinking", "/show-thinking-toggle"];
            let cmd = content.split_whitespace().next().unwrap_or(&content);
            if !known.contains(&cmd) {
                self.send_message(chat_id, "Unknown command. Available: /help, /cd, /claude, /ll, /quit, /pwd").await?;
                return Ok(());
            }
        }

        let response = session.router.handle(&content).await;

        match response {
            Some(text) => {
                self.send_message(chat_id, &text).await?;
            }
            None => {
                self.poll_claude_and_reply(chat_id, session.controller.clone()).await?;
            }
        }

        Ok(())
    }

    async fn poll_claude_and_reply(
        &self,
        chat_id: i64,
        controller: Arc<Mutex<ClaudeController>>,
    ) -> Result<()> {
        let event_rx = {
            let ctrl = controller.lock().await;
            ctrl.event_rx_clone()
        };

        let mut accumulator = EventAccumulator::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut first_text_sent = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            let event_fut = async {
                let mut rx = event_rx.lock().await;
                rx.recv().await
            };
            tokio::pin!(event_fut);

            tokio::select! {
                _ = interval.tick() => {
                    let partial = accumulator.take_output();
                    if !partial.trim().is_empty() {
                        let _ = self.send_message(chat_id, &partial).await;
                    }
                }
                event_res = tokio::time::timeout(remaining, event_fut) => {
                    match event_res {
                        Ok(Some(event)) => {
                            if let ControllerEvent::PermissionRequest(req_id, tool_name) = &event {
                                let card = format!("Permission request: `{}`\nID: `{}`", tool_name, req_id);
                                let _ = self.send_message(chat_id, &card).await;
                                continue;
                            }
                            let is_text = matches!(event, ControllerEvent::Text(_));
                            let is_done = accumulator.process_event(&event);
                            let should_flush = if !first_text_sent {
                                is_text
                            } else {
                                accumulator.peek_output().len() >= 300
                            };
                            if is_text && should_flush {
                                let partial = accumulator.take_output();
                                if !partial.trim().is_empty() {
                                    let _ = self.send_message(chat_id, &partial).await;
                                    first_text_sent = true;
                                }
                            }
                            if is_done {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }

        let reply = accumulator.take_output();
        if !reply.trim().is_empty() {
            self.send_message(chat_id, reply.trim()).await?;
        }
        Ok(())
    }

    async fn get_session(&self, chat_id: &str) -> TgChatSession {
        if let Some(session) = self.sessions.get(chat_id) {
            return session.clone();
        }
        let session = TgChatSession::new(
            self.claude_config.clone(),
            self.show_thinking,
            &self.default_dir,
        );
        self.sessions.insert(chat_id.to_string(), session.clone());
        session
    }

    fn is_allowed_sender(&self, user_id: i64, username: &str) -> bool {
        if self.config.allow_from == "*" {
            return true;
        }
        self.config
            .allow_from
            .split(',')
            .any(|s| {
                let s = s.trim();
                s == user_id.to_string() || s == username
            })
    }

    pub async fn shutdown_all_sessions(&self) {
        for entry in self.sessions.iter() {
            let chat_id = entry.key().clone();
            let session = entry.value().clone();
            drop(entry);

            if let Ok(chat_id_i64) = chat_id.parse::<i64>() {
                let _ = self.send_message(chat_id_i64, "机器人正在关闭，会话已退出").await;
            }

            let ctrl = session.controller.lock().await;
            match tokio::time::timeout(
                Duration::from_millis(500),
                ctrl.stop_session(),
            ).await {
                Ok(Ok(())) => info!("[Telegram] Session {} stopped gracefully", chat_id),
                Ok(Err(e)) => warn!("[Telegram] Session {} stop error: {}", chat_id, e),
                Err(_) => warn!("[Telegram] Session {} stop timed out, killing", chat_id),
            }
        }
    }
}

#[async_trait::async_trait]
impl Platform for TelegramPlatform {
    async fn run(&self) -> Result<()> {
        info!("Starting Telegram platform...");
        loop {
            match self.get_updates().await {
                Ok(updates) => {
                    for update in updates {
                        let next_offset = update.update_id + 1;
                        self.offset.store(next_offset, Ordering::SeqCst);

                        if let Err(e) = self.handle_update(update).await {
                            error!("[Telegram] Failed to handle update: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("[Telegram] getUpdates error: {}", e);
                    sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }

    async fn shutdown(&self) {
        self.shutdown_all_sessions().await;
    }
}

// ---------------------------------------------------------------------------
// Telegram API types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
struct Update {
    #[serde(rename = "update_id")]
    update_id: i64,
    message: Option<Message>,
}

#[derive(Debug, Clone, Deserialize)]
struct Message {
    #[serde(rename = "message_id")]
    message_id: i64,
    from: Option<User>,
    chat: Chat,
    text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct User {
    id: i64,
    #[serde(default)]
    username: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct Chat {
    id: i64,
    #[serde(rename = "type")]
    chat_type: String,
}
