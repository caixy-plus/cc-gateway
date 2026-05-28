use anyhow::Result;
use dashmap::DashMap;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::command::router::CommandRouter;
use crate::config::model::{AgentProfiles, TelegramConfig};
use crate::platform::Platform;
use crate::runtime::controller::AgentController;
use crate::runtime::event_poller::EventPollSink;
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};
use crate::session::channel_manager::{ActiveAgentRuntime, GLOBAL_CHANNEL_SESSIONS};
use crate::t_fmt;

mod inbound;
use inbound::InboundContent;

// ---------------------------------------------------------------------------
// Output buffering policy (Telegram)
// ---------------------------------------------------------------------------

const TG_FLUSH_INTERVAL_MS: u64 = 200;
const TG_MAX_BUFFER_CHARS: usize = 2000;
/// Per-channel runtime for Telegram.
/// Each chat gets its own ChannelSession; active AgentSession is optional.
#[derive(Clone)]
pub(crate) struct TelegramChannelRuntime {
    pub(crate) channel_session: crate::session::channel_model::ChannelSession,
    pub(crate) active_agent: Option<ActiveAgentRuntime>,
    /// Ensures only one poll loop runs per chat at a time.
    poll_lock: Arc<Mutex<()>>,
}

#[derive(Clone)]
enum TelegramCallbackAction {
    ChangeDir { chat_id: String, path: String },
    SetChannelAgent { chat_id: String, provider: String },
    ResumeSession { chat_id: String, session_id: String },
    StartNewSession { chat_id: String, work_dir: String },
    DeleteSession { chat_id: String, session_id: String },
}

impl TelegramChannelRuntime {
    pub(crate) fn new(channel_session: crate::session::channel_model::ChannelSession) -> Self {
        Self {
            channel_session,
            active_agent: None,
            poll_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn shutdown_notice_chat_id(&self) -> Option<i64> {
        self.active_agent.as_ref()?;
        self.channel_session.channel_id.parse::<i64>().ok()
    }
}

/// Telegram-specific sink for AgentEventPoller.
struct TelegramEventSink<'a> {
    platform: &'a TelegramPlatform,
    chat_id: i64,
    chat_id_str: String,
}

#[async_trait::async_trait]
impl<'a> EventPollSink for TelegramEventSink<'a> {
    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()> {
        let _ = is_done;
        if text.trim().is_empty() {
            return Ok(());
        }
        self.platform.send_message(self.chat_id, text).await?;
        crate::web::state::broadcast_event(
            &self.chat_id_str,
            "telegram",
            &self.chat_id_str,
            "assistant",
            text,
        );
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        _input: Option<&serde_json::Value>,
    ) -> Result<()> {
        let card = crate::t_fmt!(
            "telegram.permission_request",
            NAME = tool_name,
            ID = request_id
        );
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
        _questions: &[crate::runtime::controller::QuestionItem],
    ) -> Result<()> {
        // Telegram does not support interactive questions yet; just acknowledge.
        Ok(())
    }
}

#[derive(Clone)]
pub struct TelegramPlatform {
    config: TelegramConfig,
    default_dir: String,
    agent_settings: AgentProfiles,
    show_thinking: bool,
    http_client: reqwest::Client,
    channels: Arc<DashMap<String, TelegramChannelRuntime>>,
    offset: Arc<AtomicI64>,
    callbacks: Arc<DashMap<String, TelegramCallbackAction>>,
    callback_counter: Arc<AtomicU64>,
}

impl TelegramPlatform {
    fn telegram_commands() -> Vec<(&'static str, &'static str)> {
        vec![
            ("help", crate::t!("telegram.command_help")),
            ("pwd", crate::t!("telegram.command_pwd")),
            ("ll", crate::t!("telegram.command_ll")),
            ("cd", crate::t!("telegram.command_cd")),
            ("cd_up", crate::t!("telegram.command_cd_up")),
            ("cd_default", crate::t!("telegram.command_cd_default")),
            ("mkdir", crate::t!("telegram.command_mkdir")),
            ("agent", crate::t!("telegram.command_agent")),
            ("agents", crate::t!("telegram.command_agents")),
            ("agent_claude", crate::t!("telegram.command_agent_claude")),
            ("agent_cursor", crate::t!("telegram.command_agent_cursor")),
            ("agents_claude", crate::t!("telegram.command_agents_claude")),
            ("agents_cursor", crate::t!("telegram.command_agents_cursor")),
            ("agent_history", crate::t!("telegram.command_agent_history")),
            ("show_thinking", crate::t!("telegram.command_show_thinking")),
            ("hide_thinking", crate::t!("telegram.command_hide_thinking")),
            ("quit", crate::t!("telegram.command_quit")),
        ]
    }

    fn telegram_help_text() -> String {
        let mut out = String::new();
        out.push_str(crate::t!("telegram.help_title"));
        out.push('\n');
        for (cmd, desc) in Self::telegram_commands() {
            out.push('/');
            out.push_str(cmd);
            out.push_str("  ");
            out.push_str(desc);
            out.push('\n');
        }
        out.push('\n');
        out.push_str(crate::t!("telegram.help_footer"));
        out
    }

    pub fn new<C: Into<AgentProfiles>>(
        config: TelegramConfig,
        default_dir: &str,
        agent_settings: C,
        show_thinking: bool,
    ) -> Self {
        Self {
            config,
            default_dir: default_dir.to_string(),
            agent_settings: agent_settings.into(),
            show_thinking,
            http_client: reqwest::Client::new(),
            channels: Arc::new(DashMap::new()),
            offset: Arc::new(AtomicI64::new(0)),
            callbacks: Arc::new(DashMap::new()),
            callback_counter: Arc::new(AtomicU64::new(1)),
        }
    }

    pub(crate) fn api_url(&self, method: &str) -> String {
        format!(
            "https://api.telegram.org/bot{}/{}",
            self.config.bot_token, method
        )
    }

    pub(crate) fn mcp_context_for_chat(
        &self,
        chat_id: &str,
    ) -> crate::runtime::mcp_server::McpContext {
        crate::runtime::mcp_server::McpContext {
            delivery: crate::runtime::file_delivery::McpDeliveryTarget::Telegram(
                crate::runtime::file_delivery::TelegramFileTarget {
                    bot_token: self.config.bot_token.clone(),
                    chat_id: chat_id.to_string(),
                },
            ),
        }
    }

    pub(crate) fn bot_commands_payload() -> Value {
        json!({
            "commands": Self::telegram_commands()
                .into_iter()
                .map(|(cmd, desc)| json!({ "command": cmd, "description": desc }))
                .collect::<Vec<_>>()
        })
    }

    async fn set_bot_commands(&self) -> Result<()> {
        let url = self.api_url("setMyCommands");
        let resp = self
            .http_client
            .post(&url)
            .json(&Self::bot_commands_payload())
            .send()
            .await?;

        if !resp.status().is_success() {
            let body = resp.text().await?;
            anyhow::bail!("Telegram setMyCommands failed: {}", body);
        }
        Ok(())
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
            let desc = body
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Telegram API error: {}", desc);
        }

        let updates: Vec<Update> =
            serde_json::from_value(body.get("result").cloned().unwrap_or(json!([])))?;
        Ok(updates)
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        for (i, chunk) in split_text_into_chunks(text, 3500).iter().enumerate() {
            if i > 0 {
                sleep(Duration::from_millis(200)).await;
            }
            let url = self.api_url("sendMessage");
            let payload = json!({
                "chat_id": chat_id,
                "text": chunk,
            });

            let resp = self.http_client.post(&url).json(&payload).send().await?;

            if !resp.status().is_success() {
                let body = resp.text().await?;
                anyhow::bail!("Telegram sendMessage failed: {}", body);
            }
        }
        Ok(())
    }

    async fn send_message_with_markup(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Value,
    ) -> Result<()> {
        let url = self.api_url("sendMessage");
        let payload = json!({
            "chat_id": chat_id,
            "text": text,
            "reply_markup": reply_markup,
        });

        let resp = self.http_client.post(&url).json(&payload).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await?;
            anyhow::bail!("Telegram sendMessage failed: {}", body);
        }
        Ok(())
    }

    async fn answer_callback_query(&self, callback_id: &str, text: Option<&str>) -> Result<()> {
        let url = self.api_url("answerCallbackQuery");
        let mut payload = json!({
            "callback_query_id": callback_id,
        });
        if let Some(text) = text {
            payload["text"] = json!(text);
        }

        let resp = self.http_client.post(&url).json(&payload).send().await?;

        if !resp.status().is_success() {
            let body = resp.text().await?;
            anyhow::bail!("Telegram answerCallbackQuery failed: {}", body);
        }
        Ok(())
    }

    fn register_callback(&self, action: TelegramCallbackAction) -> String {
        let id = self.callback_counter.fetch_add(1, Ordering::SeqCst);
        let token = format!("cg:{}", id);
        self.callbacks.insert(token.clone(), action);
        token
    }

    pub(crate) fn directory_reply_markup(&self, chat_id: &str, dirs: &[(String, String)]) -> Value {
        let rows: Vec<Value> = dirs
            .iter()
            .map(|(name, path)| {
                let callback_data = self.register_callback(TelegramCallbackAction::ChangeDir {
                    chat_id: chat_id.to_string(),
                    path: path.clone(),
                });
                json!([{
                    "text": format!("{}/", name),
                    "callback_data": callback_data,
                }])
            })
            .collect();

        json!({ "inline_keyboard": rows })
    }

    pub(crate) fn agent_reply_markup(
        &self,
        chat_id: &str,
        options: &[(String, String)],
        current: &crate::config::model::AgentProvider,
    ) -> Value {
        let rows: Vec<Value> = options
            .iter()
            .map(|(provider_id, display_name)| {
                let callback_data =
                    self.register_callback(TelegramCallbackAction::SetChannelAgent {
                        chat_id: chat_id.to_string(),
                        provider: provider_id.clone(),
                    });
                let label = if provider_id == &current.to_string() {
                    format!("{} *", display_name)
                } else {
                    display_name.clone()
                };
                json!([{
                    "text": label,
                    "callback_data": callback_data,
                }])
            })
            .collect();
        json!({ "inline_keyboard": rows })
    }

    pub(crate) fn history_reply_markup(
        &self,
        chat_id: &str,
        sessions: &[crate::session::channel_model::AgentSession],
    ) -> Value {
        let rows: Vec<Value> = sessions
            .iter()
            .map(|session| {
                let resume_callback =
                    self.register_callback(TelegramCallbackAction::ResumeSession {
                        chat_id: chat_id.to_string(),
                        session_id: session.id.clone(),
                    });
                let new_callback =
                    self.register_callback(TelegramCallbackAction::StartNewSession {
                        chat_id: chat_id.to_string(),
                        work_dir: session.work_dir.clone(),
                    });
                let delete_callback =
                    self.register_callback(TelegramCallbackAction::DeleteSession {
                        chat_id: chat_id.to_string(),
                        session_id: session.id.clone(),
                    });
                json!([{
                    "text": crate::t!("telegram.resume"),
                    "callback_data": resume_callback,
                }, {
                    "text": crate::t!("telegram.start_new_session"),
                    "callback_data": new_callback,
                }, {
                    "text": crate::t!("telegram.delete_session"),
                    "callback_data": delete_callback,
                }])
            })
            .collect();

        json!({ "inline_keyboard": rows })
    }

    pub(crate) fn history_message_text(
        sessions: &[crate::session::channel_model::AgentSession],
    ) -> String {
        let china_tz = chrono::FixedOffset::east_opt(8 * 3600).unwrap();
        let mut lines = vec![crate::t!("telegram.session_history_subtitle").to_string()];

        for (idx, session) in sessions.iter().enumerate() {
            let status_dot = if session.active {
                "\u{1F7E2}"
            } else {
                "\u{26AA}"
            };
            let time = session
                .created_at
                .with_timezone(&china_tz)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            lines.push(String::new());
            lines.push(format!("{}. {} {}", idx + 1, status_dot, session.title));
            lines.push(format!("\u{1F4C1} {}", session.work_dir));
            lines.push(format!("\u{1F552} {}", time));
            if let Some(ref session_id) = session.provider_session_id {
                lines.push(format!("\u{1F511} {}", session_id));
            }
        }

        lines.join("\n")
    }

    async fn handle_update(&self, update: Update) -> Result<()> {
        if let Some(callback_query) = update.callback_query {
            return self.handle_callback_query(callback_query).await;
        }

        let msg = match update.message {
            Some(m) => m,
            None => return Ok(()),
        };

        let chat_id = msg.chat.id;
        let user_id = msg.from.as_ref().map(|u| u.id).unwrap_or(0);
        let username = msg
            .from
            .as_ref()
            .and_then(|u| u.username.clone())
            .unwrap_or_default();

        if !self.is_allowed_sender(user_id, &username) {
            debug!(
                "Telegram message from unauthorized user: {} (@{})",
                user_id, username
            );
            return Ok(());
        }

        let content = match self
            .resolve_inbound_content(InboundContent {
                text: msg.text.as_deref(),
                caption: msg.caption.as_deref(),
                photo: msg.photo.as_deref(),
                document: msg.document.as_ref(),
                video: msg.video.as_ref(),
                audio: msg.audio.as_ref(),
                voice: msg.voice.as_ref(),
            })
            .await
        {
            Ok(content) => content,
            Err(e) => {
                warn!("Telegram failed to resolve inbound media: {}", e);
                self.send_message(chat_id, &crate::t_fmt!("telegram.error_generic", ERR = e))
                    .await?;
                return Ok(());
            }
        };
        if content.trim().is_empty() {
            return Ok(());
        }

        // Telegram-specific help: do not reuse the generic TUI/CLI help text.
        // (e.g. /help@botname is also common)
        let cmd = content.split_whitespace().next().unwrap_or("");
        if cmd == "/help" || cmd.starts_with("/help@") {
            self.send_message(chat_id, &Self::telegram_help_text())
                .await?;
            return Ok(());
        }

        // Only allow private chats. Group chats are not supported.
        if msg.chat.chat_type != "private" {
            self.send_message(chat_id, crate::t!("telegram.private_chat_only"))
                .await?;
            return Ok(());
        }

        let chat_id_str = chat_id.to_string();
        let runtime = self.get_channel(&chat_id_str).await;

        // Build a router for this channel
        let router = if let Some(ref active) = runtime.active_agent {
            CommandRouter::new(active.controller.clone(), &self.default_dir)
        } else {
            // No active session: create a temporary router with a dummy controller
            // so that route() can still classify commands correctly.
            let dummy = Arc::new(Mutex::new(AgentController::new(
                self.agent_settings.clone(),
                self.show_thinking,
            )));
            CommandRouter::new(dummy, &self.default_dir)
        };

        let action = router.route(&content).await;
        let executor = ChatCommandExecutor::new(
            &self.default_dir,
            self.agent_settings.clone(),
            self.show_thinking,
        );
        let mut context = ChatCommandContext::new(
            runtime.channel_session.id.clone(),
            format!("Telegram {}", chat_id),
            runtime.channel_session.work_dir.clone(),
            runtime.active_agent.clone(),
        )
        .with_mcp_context(self.mcp_context_for_chat(&chat_id_str));
        let outcome = executor.execute(&mut context, action).await?;

        if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
            rt.active_agent = context.active_agent.clone();
            rt.channel_session.work_dir = context.channel_work_dir.clone();
        }

        match outcome {
            ChatCommandOutcome::Reply(text)
            | ChatCommandOutcome::Error(text)
            | ChatCommandOutcome::Stopped { message: text }
            | ChatCommandOutcome::ThinkingShown { message: text }
            | ChatCommandOutcome::ThinkingHidden { message: text } => {
                self.send_message(chat_id, &text).await?;
            }
            ChatCommandOutcome::WorkDirChanged { work_dir, message }
            | ChatCommandOutcome::CurrentDir { work_dir, message } => {
                let _ = work_dir;
                self.send_message(chat_id, &message).await?;
            }
            ChatCommandOutcome::DirCreated { path, message } => {
                let _ = path;
                self.send_message(chat_id, &message).await?;
            }
            ChatCommandOutcome::NoOp => {}
            ChatCommandOutcome::SelectAgent { current, options } => {
                let markup = self.agent_reply_markup(&chat_id_str, &options, &current);
                self.send_message_with_markup(chat_id, crate::t!("telegram.choose_agent"), markup)
                    .await?;
            }
            ChatCommandOutcome::ListDir { dir, dirs } => {
                if dirs.is_empty() {
                    self.send_message(chat_id, crate::t!("builtin.no_subdirs"))
                        .await?;
                } else {
                    let markup = self.directory_reply_markup(&chat_id_str, &dirs);
                    self.send_message_with_markup(
                        chat_id,
                        &crate::t_fmt!("telegram.choose_directory", DIR = dir),
                        markup,
                    )
                    .await?;
                }
            }
            ChatCommandOutcome::Started { message, .. } => {
                self.send_message(chat_id, &message).await?;
            }
            ChatCommandOutcome::History { sessions } => {
                if sessions.is_empty() {
                    self.send_message(chat_id, crate::t!("feishu.no_sessions"))
                        .await?;
                } else {
                    let markup = self.history_reply_markup(&chat_id_str, &sessions);
                    self.send_message_with_markup(
                        chat_id,
                        &Self::history_message_text(&sessions),
                        markup,
                    )
                    .await?;
                }
            }
            ChatCommandOutcome::ForwardToAgent { active, text } => {
                let _guard = runtime.poll_lock.lock().await;
                let sink = TelegramEventSink {
                    platform: self,
                    chat_id,
                    chat_id_str: chat_id_str.clone(),
                };
                // Telegram: time-first flush (200ms), buffer max 2000 chars.
                let mut sink = crate::runtime::event_poller::BufferedSink::new(
                    sink,
                    std::time::Duration::from_millis(TG_FLUSH_INTERVAL_MS),
                    TG_MAX_BUFFER_CHARS,
                );
                GLOBAL_CHANNEL_SESSIONS
                    .send_and_poll_active_runtime_buffered(&active, &text, &mut sink)
                    .await?;
            }
        }

        Ok(())
    }

    async fn handle_callback_query(&self, callback_query: CallbackQuery) -> Result<()> {
        let Some(message) = callback_query.message else {
            let _ = self
                .answer_callback_query(
                    &callback_query.id,
                    Some(crate::t!("telegram.callback_expired")),
                )
                .await;
            return Ok(());
        };
        let Some(data) = callback_query.data else {
            let _ = self.answer_callback_query(&callback_query.id, None).await;
            return Ok(());
        };

        let chat_id = message.chat.id;
        let chat_id_str = chat_id.to_string();
        let user_id = callback_query.from.id;
        let username = callback_query.from.username.unwrap_or_default();

        if !self.is_allowed_sender(user_id, &username) {
            debug!(
                "Telegram callback from unauthorized user: {} (@{})",
                user_id, username
            );
            let _ = self
                .answer_callback_query(
                    &callback_query.id,
                    Some(crate::t!("telegram.callback_expired")),
                )
                .await;
            return Ok(());
        }

        if message.chat.chat_type != "private" {
            self.send_message(chat_id, crate::t!("telegram.private_chat_only"))
                .await?;
            return Ok(());
        }

        let Some((_, action)) = self.callbacks.remove(&data) else {
            let _ = self
                .answer_callback_query(
                    &callback_query.id,
                    Some(crate::t!("telegram.callback_expired")),
                )
                .await;
            return Ok(());
        };

        match action {
            TelegramCallbackAction::ChangeDir {
                chat_id: action_chat_id,
                path,
            } => {
                if action_chat_id != chat_id_str {
                    let _ = self
                        .answer_callback_query(
                            &callback_query.id,
                            Some(crate::t!("telegram.callback_expired")),
                        )
                        .await;
                    return Ok(());
                }

                let runtime = self.get_channel(&chat_id_str).await;
                GLOBAL_CHANNEL_SESSIONS
                    .switch_work_dir(&runtime.channel_session.id, std::path::PathBuf::from(&path))
                    .await?;
                if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                    rt.channel_session.work_dir = path.clone();
                }
                let _ = self.answer_callback_query(&callback_query.id, None).await;
                self.send_message(chat_id, &crate::t_fmt!("builtin.dir_changed", PATH = path))
                    .await?;
            }
            TelegramCallbackAction::SetChannelAgent {
                chat_id: action_chat_id,
                provider,
            } => {
                if action_chat_id != chat_id_str {
                    let _ = self
                        .answer_callback_query(
                            &callback_query.id,
                            Some(crate::t!("telegram.callback_expired")),
                        )
                        .await;
                    return Ok(());
                }
                let runtime = self.get_channel(&chat_id_str).await;
                let provider = crate::config::model::AgentProvider::parse_str(&provider);
                let name = crate::command::agents::provider_display_name(&provider);
                match GLOBAL_CHANNEL_SESSIONS
                    .set_channel_default_provider(&runtime.channel_session.id, provider)
                {
                    Ok(()) => {
                        let _ = self.answer_callback_query(&callback_query.id, None).await;
                        self.send_message(
                            chat_id,
                            &crate::t_fmt!("builtin.channel_agent_set", NAME = name),
                        )
                        .await?;
                    }
                    Err(e) => {
                        let _ = self
                            .answer_callback_query(
                                &callback_query.id,
                                Some(&t_fmt!("builtin.failed_set_channel_agent", ERR = e)),
                            )
                            .await;
                    }
                }
            }
            TelegramCallbackAction::ResumeSession {
                chat_id: action_chat_id,
                session_id,
            } => {
                if action_chat_id != chat_id_str {
                    let _ = self
                        .answer_callback_query(
                            &callback_query.id,
                            Some(crate::t!("telegram.callback_expired")),
                        )
                        .await;
                    return Ok(());
                }

                let runtime = self.get_channel(&chat_id_str).await;
                let active = GLOBAL_CHANNEL_SESSIONS
                    .resume_agent_session_for_platform(
                        &session_id,
                        &self.default_dir,
                        self.agent_settings.clone(),
                        self.show_thinking,
                        Some(runtime.channel_session.work_dir.clone()),
                        Some(self.mcp_context_for_chat(&chat_id_str)),
                    )
                    .await?;
                let provider = active.agent_session.stored_provider();
                let work_dir = active.agent_session.work_dir.clone();
                if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                    rt.channel_session.work_dir = work_dir.clone();
                    rt.active_agent = Some(active);
                }
                let _ = self.answer_callback_query(&callback_query.id, None).await;
                self.send_message(
                    chat_id,
                    &crate::command::agents::session_resumed_message(&provider, &work_dir),
                )
                .await?;
            }
            TelegramCallbackAction::StartNewSession {
                chat_id: action_chat_id,
                work_dir,
            } => {
                if action_chat_id != chat_id_str {
                    let _ = self
                        .answer_callback_query(
                            &callback_query.id,
                            Some(crate::t!("telegram.callback_expired")),
                        )
                        .await;
                    return Ok(());
                }

                let runtime = self.get_channel(&chat_id_str).await;
                let provider = GLOBAL_CHANNEL_SESSIONS
                    .effective_channel_provider(&runtime.channel_session.id, &self.agent_settings);
                let active = GLOBAL_CHANNEL_SESSIONS
                    .start_agent_session_for_platform(
                        crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                            channel_id: runtime.channel_session.id.clone(),
                            title: format!("Telegram {}", chat_id),
                            default_dir: self.default_dir.clone(),
                            agent_settings: self.agent_settings.clone(),
                            show_thinking: self.show_thinking,
                            args: vec![],
                            resume_session_id: None,
                            work_dir_override: Some(work_dir),
                            mcp_context: Some(self.mcp_context_for_chat(&chat_id_str)),
                            provider_override: Some(provider),
                        },
                    )
                    .await?;
                let started_provider = active.agent_session.stored_provider();
                let work_dir = active.agent_session.work_dir.clone();
                if let Some(mut rt) = self.channels.get_mut(&chat_id_str) {
                    rt.channel_session.work_dir = work_dir.clone();
                    rt.active_agent = Some(active);
                }
                let _ = self.answer_callback_query(&callback_query.id, None).await;
                self.send_message(
                    chat_id,
                    &crate::command::agents::session_started_message(&started_provider, &work_dir),
                )
                .await?;
            }
            TelegramCallbackAction::DeleteSession {
                chat_id: action_chat_id,
                session_id,
            } => {
                if action_chat_id != chat_id_str {
                    let _ = self
                        .answer_callback_query(
                            &callback_query.id,
                            Some(crate::t!("telegram.callback_expired")),
                        )
                        .await;
                    return Ok(());
                }

                let message = if GLOBAL_CHANNEL_SESSIONS.remove_agent_session(&session_id) {
                    crate::t!("telegram.session_deleted")
                } else {
                    crate::t!("telegram.cannot_delete_active")
                };
                let _ = self.answer_callback_query(&callback_query.id, None).await;
                self.send_message(chat_id, message).await?;
            }
        }

        Ok(())
    }

    pub(crate) async fn get_channel(&self, chat_id: &str) -> TelegramChannelRuntime {
        if let Some(runtime) = self.channels.get(chat_id) {
            return runtime.clone();
        }
        let channel = GLOBAL_CHANNEL_SESSIONS
            .get_or_create_platform_channel("telegram", chat_id, &self.default_dir)
            .await;
        let runtime = TelegramChannelRuntime::new(channel);
        self.channels.insert(chat_id.to_string(), runtime.clone());
        runtime
    }

    pub(crate) fn is_allowed_sender(&self, user_id: i64, username: &str) -> bool {
        if self.config.allow_from == "*" {
            return true;
        }
        self.config.allow_from.split(',').any(|s| {
            let s = s.trim();
            s == user_id.to_string() || s == username
        })
    }

    pub async fn shutdown_all_sessions(&self) {
        for entry in self.channels.iter() {
            let chat_id = entry.key().clone();
            let runtime = entry.value().clone();
            drop(entry);

            if let Some(chat_id_i64) = runtime.shutdown_notice_chat_id() {
                let _ = self
                    .send_message(chat_id_i64, crate::t!("telegram.shutdown_notice"))
                    .await;
            }

            if let Some(ref active) = runtime.active_agent {
                let ctrl = active.controller.lock().await;
                match tokio::time::timeout(Duration::from_millis(500), ctrl.stop_session()).await {
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

fn split_text_into_chunks(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let line_len = line.chars().count();
        if line_len > max_chars {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            let mut remaining = line;
            while !remaining.is_empty() {
                let split_at = remaining
                    .char_indices()
                    .take(max_chars)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(remaining.len());
                chunks.push(remaining[..split_at].to_string());
                remaining = &remaining[split_at..];
            }
        } else if !current.is_empty() && current.chars().count() + line_len + 1 > max_chars {
            chunks.push(std::mem::take(&mut current));
            current.push_str(line);
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[async_trait::async_trait]
impl Platform for TelegramPlatform {
    async fn run(&self) -> Result<()> {
        info!("Starting Telegram platform...");
        if let Err(e) = self.set_bot_commands().await {
            warn!("[Telegram] Failed to set bot commands: {}", e);
        }
        self.spawn_deliver_listener();
        loop {
            match self.get_updates().await {
                Ok(updates) => {
                    for update in updates {
                        let next_offset = update.update_id + 1;
                        self.offset.store(next_offset, Ordering::SeqCst);
                        // Handle updates concurrently so a long-running agent turn
                        // does not block urgent control commands like /quit.
                        let platform = self.clone();
                        tokio::spawn(async move {
                            if let Err(e) = platform.handle_update(update).await {
                                error!("[Telegram] Failed to handle update: {}", e);
                            }
                        });
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
    callback_query: Option<CallbackQuery>,
}

#[derive(Debug, Clone, Deserialize)]
struct Message {
    #[serde(rename = "message_id")]
    #[allow(dead_code)]
    message_id: i64,
    from: Option<User>,
    chat: Chat,
    text: Option<String>,
    caption: Option<String>,
    photo: Option<Vec<inbound::TelegramPhotoSize>>,
    document: Option<inbound::TelegramFileRef>,
    video: Option<inbound::TelegramFileRef>,
    audio: Option<inbound::TelegramFileRef>,
    voice: Option<inbound::TelegramVoice>,
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

#[derive(Debug, Clone, Deserialize)]
struct CallbackQuery {
    id: String,
    from: User,
    message: Option<CallbackMessage>,
    data: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct CallbackMessage {
    chat: Chat,
}
