use anyhow::Result;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::claude::controller::ClaudeController;
use crate::claude::event_poller::{ClaudeEventPoller, EventPollSink};
use crate::command::router::{CommandAction, CommandRouter};
use crate::config::model::{ClaudeConfig, TelegramConfig};
use crate::platform::Platform;
use crate::session::channel_manager::{ActiveClaudeRuntime, GLOBAL_CHANNEL_SESSIONS};
/// Per-channel runtime for Telegram.
/// Each chat gets its own ChannelSession; active ClaudeSession is optional.
#[derive(Clone)]
struct TelegramChannelRuntime {
    channel_session: crate::session::channel_model::ChannelSession,
    active_claude: Option<ActiveClaudeRuntime>,
    /// Ensures only one poll loop runs per chat at a time.
    poll_lock: Arc<Mutex<()>>,
}

impl TelegramChannelRuntime {
    fn new(channel_session: crate::session::channel_model::ChannelSession) -> Self {
        Self {
            channel_session,
            active_claude: None,
            poll_lock: Arc::new(Mutex::new(())),
        }
    }
}

/// Telegram-specific sink for ClaudeEventPoller.
struct TelegramEventSink<'a> {
    platform: &'a TelegramPlatform,
    chat_id: i64,
    chat_id_str: String,
}

#[async_trait::async_trait]
impl<'a> EventPollSink for TelegramEventSink<'a> {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        if !text.trim().is_empty() {
            self.platform.send_message(self.chat_id, text).await?;
            crate::web::state::broadcast_event(
                &self.chat_id_str,
                "telegram",
                &self.chat_id_str,
                "assistant",
                text,
            );
        }
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        _input: Option<&serde_json::Value>,
    ) -> Result<()> {
        let card = format!("Permission request: `{}`\nID: `{}`", tool_name, request_id);
        self.platform.send_message(self.chat_id, &card).await?;
        crate::web::state::broadcast_event(
            &self.chat_id_str,
            "telegram",
            &self.chat_id_str,
            "system",
            &card,
        );
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        _request_id: &str,
        prompt: &str,
        _options: &[String],
    ) -> Result<()> {
        self.platform.send_message(self.chat_id, prompt).await?;
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        _request_id: &str,
        prompt: &str,
        _options: &[String],
    ) -> Result<()> {
        self.platform.send_message(self.chat_id, prompt).await?;
        Ok(())
    }

    async fn on_question_request(
        &mut self,
        _request_id: &str,
        _questions: &[crate::claude::controller::QuestionItem],
    ) -> Result<()> {
        // Telegram does not support interactive questions yet; just acknowledge.
        Ok(())
    }

}

#[derive(Clone)]
pub struct TelegramPlatform {
    config: TelegramConfig,
    default_dir: String,
    claude_config: ClaudeConfig,
    show_thinking: bool,
    http_client: reqwest::Client,
    channels: Arc<DashMap<String, TelegramChannelRuntime>>,
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
            channels: Arc::new(DashMap::new()),
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

        // Only allow private chats. Group chats are not supported.
        if msg.chat.chat_type != "private" {
            self.send_message(chat_id, "Please use in private chat.").await?;
            return Ok(());
        }

        let chat_id_str = chat_id.to_string();
        let runtime = self.get_channel(&chat_id_str).await;

        // Build a router for this channel
        let router = if let Some(ref active) = runtime.active_claude {
            CommandRouter::new(active.controller.clone(), &self.default_dir)
        } else {
            // No active session: create a temporary router with a dummy controller
            // so that route() can still classify commands correctly.
            let dummy = Arc::new(Mutex::new(ClaudeController::new(
                self.claude_config.clone(),
                self.show_thinking,
            )));
            CommandRouter::new(dummy, &self.default_dir)
        };

        let action = router.route(&content).await;

        match action {
            CommandAction::Reply(text) => {
                self.send_message(chat_id, &text).await?;
            }
            CommandAction::UnknownCommand(_) => {
                let text = "Unknown command. Available: /help, /cd, /claude, /ll, /list-sessions, /switch-session, /quit, /pwd".to_string();
                self.send_message(chat_id, &text).await?;
            }
            CommandAction::NoOp => {}
            CommandAction::StopSession => {
                let _ = GLOBAL_CHANNEL_SESSIONS
                    .stop_channel_session(&runtime.channel_session.id)
                    .await;
                if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                    rt.active_claude = None;
                }
                self.send_message(chat_id, "Session stopped.").await?;
            }
            CommandAction::ShowThinking => {
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(true);
                }
                self.send_message(chat_id, "Thinking enabled").await?;
            }
            CommandAction::HideThinking => {
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(false);
                }
                self.send_message(chat_id, "Thinking disabled").await?;
            }
            CommandAction::ChangeDir(path) => {
                let base = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                let target = PathBuf::from(&base).join(&path);
                let canonical = target.canonicalize().unwrap_or(target);
                if !canonical.is_dir() {
                    self.send_message(chat_id, &format!("Invalid path: {}", canonical.display()))
                        .await?;
                    return Ok(());
                }
                let path_str = canonical.to_string_lossy().to_string();
                if let Err(e) = crate::claude::controller::ensure_under_home(&path_str) {
                    self.send_message(chat_id, &e.to_string()).await?;
                    return Ok(());
                }
                GLOBAL_CHANNEL_SESSIONS
                    .switch_work_dir(&runtime.channel_session.id, canonical)
                    .await?;
                if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                    rt.channel_session.work_dir = path_str.clone();
                }
                self.send_message(chat_id, &format!("Working directory changed to: {}", path_str))
                    .await?;
            }
            CommandAction::ChangeDirDefault => {
                let dir = shellexpand::tilde(&self.default_dir).to_string();
                GLOBAL_CHANNEL_SESSIONS
                    .switch_work_dir(&runtime.channel_session.id, PathBuf::from(&dir))
                    .await?;
                if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                    rt.channel_session.work_dir = dir.clone();
                }
                self.send_message(chat_id, &format!("Working directory changed to: {}", dir))
                    .await?;
            }
            CommandAction::PrintWorkingDir => {
                let dir = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                self.send_message(chat_id, &format!("Current directory: {}", dir))
                    .await?;
            }
            CommandAction::ListDir { path } => {
                let dir = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                let target = path.unwrap_or_else(|| PathBuf::from(&dir));
                match crate::command::builtin::list_directory_paths(&target.to_string_lossy()) {
                    Ok(dirs) => {
                        if dirs.is_empty() {
                            self.send_message(chat_id, "No subdirectories found.").await?;
                        } else {
                            let lines: Vec<String> =
                                dirs.iter().map(|(name, _)| format!("  {}/", name)).collect();
                            self.send_message(
                                chat_id,
                                &format!("Directories in {}:\n{}", dir, lines.join("\n")),
                            )
                            .await?;
                        }
                    }
                    Err(e) => {
                        self.send_message(chat_id, &format!("Failed to list directory: {}", e))
                            .await?;
                    }
                }
            }
            CommandAction::MakeDir(path) => {
                let base = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                let target = PathBuf::from(&base).join(&path);
                let target_str = target.to_string_lossy().to_string();
                if let Err(e) = crate::claude::controller::ensure_under_home(&target_str) {
                    self.send_message(chat_id, &e.to_string()).await?;
                    return Ok(());
                }
                match std::fs::create_dir_all(&target) {
                    Ok(()) => {
                        self.send_message(
                            chat_id,
                            &format!("Directory created: {}", target_str),
                        )
                        .await?;
                    }
                    Err(e) => {
                        self.send_message(
                            chat_id,
                            &format!("Failed to create directory: {}", e),
                        )
                        .await?;
                    }
                }
            }
            CommandAction::StartSession { work_dir, args } => {
                let effective_dir = work_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| {
                        if runtime.channel_session.work_dir.is_empty() {
                            shellexpand::tilde(&self.default_dir).to_string()
                        } else {
                            runtime.channel_session.work_dir.clone()
                        }
                    });

                match GLOBAL_CHANNEL_SESSIONS
                    .create_and_start_claude_session(
                        &runtime.channel_session.id,
                        &format!("Telegram {}", chat_id),
                        self.claude_config.clone(),
                        self.show_thinking,
                        args,
                        None,
                        None,
                    )
                    .await
                {
                    Ok((claude_session, controller)) => {
                        // Sync work_dir if different
                        if effective_dir != claude_session.work_dir {
                            let ctrl = controller.lock().await;
                            ctrl.init_work_dir(effective_dir.clone()).await;
                        }
                        let router =
                            Arc::new(CommandRouter::new(controller.clone(), &self.default_dir));
                        let active = ActiveClaudeRuntime {
                            claude_session: claude_session.clone(),
                            controller: controller.clone(),
                            router,
                        };
                        if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                            rt.active_claude = Some(active.clone());
                        }
                        self.send_message(
                            chat_id,
                            &format!("Session started in {}", claude_session.work_dir),
                        )
                        .await?;

                        let _guard = runtime.poll_lock.lock().await;
                        let mut sink = TelegramEventSink {
                            platform: self,
                            chat_id,
                            chat_id_str: chat_id_str.clone(),
                        };
                        let poller = {
                            let ctrl = controller.lock().await;
                            ClaudeEventPoller::from_controller(&*ctrl)
                        };
                        poller.run(&mut sink).await?;
                    }
                    Err(e) => {
                        self.send_message(chat_id, &format!("Failed to start session: {}", e))
                            .await?;
                    }
                }
            }
            CommandAction::ShowClaudeHistory => {
                // Not implemented for Telegram
                self.send_message(chat_id, "Claude history is not available in Telegram.")
                    .await?;
            }
            CommandAction::ForwardToClaude(text) => {
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.send_message(&text).await?;
                    drop(ctrl);

                    let _guard = runtime.poll_lock.lock().await;
                    let mut sink = TelegramEventSink {
                        platform: self,
                        chat_id,
                        chat_id_str: chat_id_str.clone(),
                    };
                    let poller = {
                        let ctrl = active.controller.lock().await;
                        ClaudeEventPoller::from_controller(&*ctrl)
                    };
                    poller.run(&mut sink).await?;
                } else {
                    self.send_message(
                        chat_id,
                        "No active Claude session. Use /claude to start one.",
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    async fn get_channel(&self, chat_id: &str) -> TelegramChannelRuntime {
        if let Some(runtime) = self.channels.get(chat_id) {
            return runtime.clone();
        }
        let channel = GLOBAL_CHANNEL_SESSIONS.get_or_create_platform_channel(
            "telegram",
            chat_id,
            &self.default_dir,
        ).await;
        let runtime = TelegramChannelRuntime::new(channel);
        self.channels.insert(chat_id.to_string(), runtime.clone());
        runtime
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
        for entry in self.channels.iter() {
            let chat_id = entry.key().clone();
            let runtime = entry.value().clone();
            drop(entry);

            if let Ok(chat_id_i64) = chat_id.parse::<i64>() {
                let _ = self.send_message(chat_id_i64, "机器人正在关闭，会话已退出").await;
            }

            if let Some(ref active) = runtime.active_claude {
                let ctrl = active.controller.lock().await;
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

    fn spawn_deliver_listener(&self) {
        let platform = self.clone();
        crate::platform::spawn_deliver_listener("telegram", move |channel_id, text| {
            let platform = platform.clone();
            tokio::spawn(async move {
                if let Ok(chat_id) = channel_id.parse::<i64>() {
                    let _ = platform.send_message(chat_id, &text).await;
                }
            });
        });
    }
}

#[async_trait::async_trait]
impl Platform for TelegramPlatform {
    async fn run(&self) -> Result<()> {
        info!("Starting Telegram platform...");
        self.spawn_deliver_listener();
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
    #[allow(dead_code)]
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
