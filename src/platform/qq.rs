//! QQ 开放平台官方机器人（WebSocket Gateway + OpenAPI v2）。

mod api;
mod ws;

use anyhow::Result;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, warn};

use crate::command::router::CommandRouter;
use crate::config::model::{AgentProfiles, QqConfig};
use crate::platform::inbound_media;
use crate::platform::Platform;
use crate::runtime::controller::AgentController;
use crate::runtime::event_poller::EventPollSink;
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};
use crate::session::channel_manager::{ActiveAgentRuntime, GLOBAL_CHANNEL_SESSIONS};
use crate::session::chat_flow;
use crate::session::outcome_text;

pub use api::{QqApiClient, QqFileChatTarget};

const QQ_FLUSH_INTERVAL_MS: u64 = 200;
const QQ_MAX_BUFFER_CHARS: usize = 2000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum QqChatTarget {
    C2c { openid: String },
    Group { group_openid: String },
}

impl QqChatTarget {
    fn channel_id(&self) -> String {
        match self {
            QqChatTarget::C2c { openid } => format!("u:{}", openid),
            QqChatTarget::Group { group_openid } => format!("g:{}", group_openid),
        }
    }

    fn title_label(&self) -> String {
        match self {
            QqChatTarget::C2c { openid } => format!("QQ DM {}", openid),
            QqChatTarget::Group { group_openid } => format!("QQ Group {}", group_openid),
        }
    }
}

#[derive(Clone)]
pub(crate) struct QqChannelRuntime {
    channel_session: crate::session::channel_model::ChannelSession,
    pub(crate) active_agent: Option<ActiveAgentRuntime>,
    chat: QqChatTarget,
    poll_lock: Arc<Mutex<()>>,
}

impl QqChannelRuntime {
    pub(crate) fn new(
        channel_session: crate::session::channel_model::ChannelSession,
        chat: QqChatTarget,
    ) -> Self {
        Self {
            channel_session,
            active_agent: None,
            chat,
            poll_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn shutdown_notice_target(&self) -> Option<QqChatTarget> {
        self.active_agent.as_ref()?;
        Some(self.chat.clone())
    }
}

#[derive(Clone)]
pub struct QqPlatform {
    config: QqConfig,
    default_dir: String,
    agent_settings: AgentProfiles,
    show_thinking: bool,
    api: QqApiClient,
    channels: Arc<DashMap<String, QqChannelRuntime>>,
}

struct QqEventSink<'a> {
    platform: &'a QqPlatform,
    chat: QqChatTarget,
    channel_id: String,
}

#[async_trait::async_trait]
impl<'a> EventPollSink for QqEventSink<'a> {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        self.platform.send_text(&self.chat, text).await?;
        crate::web::state::broadcast_event(
            &self.channel_id,
            "qq",
            &self.channel_id,
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
        let text = crate::t_fmt!("qq.permission_request", NAME = tool_name, ID = request_id);
        self.platform.send_text(&self.chat, &text).await?;
        crate::web::state::broadcast_event(
            &self.channel_id,
            "qq",
            &self.channel_id,
            "system",
            &text,
        );
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        let mut text = format!("{}\n", prompt);
        for (i, opt) in options.iter().enumerate() {
            text.push_str(&format!("{}. {}\n", i + 1, opt));
        }
        text.push_str(&format!("id: {}", request_id));
        self.platform.send_text(&self.chat, &text).await
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        self.on_confirm_request(request_id, prompt, options).await
    }

    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::runtime::controller::QuestionItem],
    ) -> Result<()> {
        let mut text = String::new();
        for q in questions {
            text.push_str(&format!("Q: {}\n", q.question));
            if !q.options.is_empty() {
                let opts: Vec<&str> = q.options.iter().map(|o| o.label.as_str()).collect();
                text.push_str(&format!("Options: {}\n", opts.join(", ")));
            }
        }
        text.push_str(&format!("id: {}", request_id));
        self.platform.send_text(&self.chat, &text).await
    }
}

impl QqPlatform {
    pub fn new<C: Into<AgentProfiles>>(
        config: QqConfig,
        default_dir: &str,
        agent_settings: C,
        show_thinking: bool,
    ) -> Self {
        let api = QqApiClient::new(
            config.app_id.clone(),
            config.app_secret.clone(),
            config.sandbox,
        );
        Self {
            config,
            default_dir: default_dir.to_string(),
            agent_settings: agent_settings.into(),
            show_thinking,
            api,
            channels: Arc::new(DashMap::new()),
        }
    }

    fn spawn_deliver_listener(&self) {
        let api = self.api.clone();
        crate::platform::spawn_deliver_listener("qq", move |channel_id, text| {
            let api = api.clone();
            tokio::spawn(async move {
                if let Some((chat, _)) = parse_channel_id(&channel_id) {
                    let _ = send_text_with_api(&api, &chat, &text).await;
                }
            });
        });
    }

    async fn send_text(&self, chat: &QqChatTarget, text: &str) -> Result<()> {
        send_text_with_api(&self.api, chat, text).await
    }

    fn mcp_context_for_chat(&self, chat: &QqChatTarget) -> crate::runtime::mcp_server::McpContext {
        crate::runtime::mcp_server::McpContext {
            delivery: crate::runtime::file_delivery::McpDeliveryTarget::Qq(
                crate::runtime::file_delivery::QqFileTarget {
                    app_id: self.config.app_id.clone(),
                    app_secret: self.config.app_secret.clone(),
                    sandbox: self.config.sandbox,
                    chat: qq_file_chat_target(chat),
                },
            ),
        }
    }

    async fn get_channel(&self, channel_id: &str) -> QqChannelRuntime {
        if let Some(rt) = self.channels.get(channel_id) {
            return rt.clone();
        }
        let chat = parse_channel_id(channel_id)
            .map(|(c, _)| c)
            .unwrap_or_else(|| QqChatTarget::C2c {
                openid: channel_id.to_string(),
            });
        let channel_session = GLOBAL_CHANNEL_SESSIONS
            .get_or_create_platform_channel("qq", channel_id, &self.default_dir)
            .await;
        let rt = QqChannelRuntime::new(channel_session, chat);
        self.channels.insert(channel_id.to_string(), rt.clone());
        rt
    }

    async fn handle_dispatch(&self, event_type: &str, data: &Value) -> Result<()> {
        match event_type {
            "C2C_MESSAGE_CREATE" => {
                let openid = data
                    .get("author")
                    .and_then(|a| a.get("user_openid").or_else(|| a.get("id")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let Some(openid) = openid else {
                    return Ok(());
                };
                self.handle_inbound(QqChatTarget::C2c { openid }, data)
                    .await
            }
            "GROUP_AT_MESSAGE_CREATE" => {
                let group_openid = data
                    .get("group_openid")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let Some(group_openid) = group_openid else {
                    return Ok(());
                };
                // Align with Feishu: disable group channel; only DM (C2C) is supported.
                let chat = QqChatTarget::Group { group_openid };
                let _ = self
                    .send_text(&chat, crate::t!("qq.group_chat_unsupported"))
                    .await;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    async fn handle_inbound(&self, chat: QqChatTarget, data: &Value) -> Result<()> {
        let user_text = api::extract_message_text(data).unwrap_or_default();
        let attachments = api::extract_inbound_attachments(data);
        if user_text.trim().is_empty() && attachments.is_empty() {
            return Ok(());
        }

        let channel_id = chat.channel_id();
        let approved =
            crate::session::pairing::GLOBAL_PAIRING_MANAGER.is_approved("qq", &channel_id);
        if !approved {
            if crate::session::pairing::GLOBAL_PAIRING_MANAGER.require_pairing("qq") {
                let code = crate::session::pairing::GLOBAL_PAIRING_MANAGER
                    .get_or_create_pending("qq", &channel_id);
                let msg = crate::t_fmt!("pairing.wait_message", CODE = code);
                self.send_text(&chat, &msg).await?;
            }
            return Ok(());
        }

        // Download inbound attachments (best-effort) into `~/.cc-gateway/media/` and forward paths.
        let mut saved = Vec::new();
        for att in attachments {
            match self.api.download_attachment(&att.url).await {
                Ok((bytes, ct)) => {
                    let item = if let Some(ref name) = att.name {
                        inbound_media::save_bytes_to_media_dir_with_upstream_name(
                            &bytes,
                            name,
                            ct.as_deref(),
                        )
                        .await
                    } else {
                        inbound_media::save_bytes_to_media_dir(&bytes, ct.as_deref()).await
                    };
                    match item {
                        Ok(it) => saved.push(it),
                        Err(e) => {
                            warn!("[QQ] Failed to save inbound attachment {}: {}", att.url, e)
                        }
                    }
                }
                Err(e) => warn!(
                    "[QQ] Failed to download inbound attachment {}: {}",
                    att.url, e
                ),
            }
        }
        let content = inbound_media::format_agent_message(&user_text, &saved);

        crate::web::state::broadcast_event(&channel_id, "qq", &channel_id, "user", &content);

        let runtime = self.get_channel(&channel_id).await;
        let router = if let Some(ref active) = runtime.active_agent {
            CommandRouter::new(active.controller.clone(), &self.default_dir)
        } else {
            let dummy = Arc::new(Mutex::new(AgentController::new(
                self.agent_settings.clone(),
                self.show_thinking,
            )));
            CommandRouter::new(dummy, &self.default_dir)
        };

        let executor = ChatCommandExecutor::new(
            &self.default_dir,
            self.agent_settings.clone(),
            self.show_thinking,
        );
        let mut context = ChatCommandContext::new(
            runtime.channel_session.id.clone(),
            chat.title_label(),
            runtime.channel_session.work_dir.clone(),
            runtime.active_agent.clone(),
        )
        .with_mcp_context(self.mcp_context_for_chat(&chat));
        let outcome =
            chat_flow::route_and_execute(&router, &executor, &mut context, &content).await?;

        if let Some(mut rt) = self.channels.get_mut(&channel_id) {
            rt.active_agent = context.active_agent.clone();
            rt.channel_session.work_dir = context.channel_work_dir.clone();
        }

        self.handle_outcome(&chat, &channel_id, runtime, outcome)
            .await
    }

    async fn handle_outcome(
        &self,
        chat: &QqChatTarget,
        channel_id: &str,
        runtime: QqChannelRuntime,
        outcome: ChatCommandOutcome,
    ) -> Result<()> {
        match outcome {
            ChatCommandOutcome::Reply(text)
            | ChatCommandOutcome::Error(text)
            | ChatCommandOutcome::Stopped { message: text }
            | ChatCommandOutcome::ThinkingShown { message: text }
            | ChatCommandOutcome::ThinkingHidden { message: text } => {
                self.send_text(chat, &text).await?;
            }
            ChatCommandOutcome::WorkDirChanged { message, .. }
            | ChatCommandOutcome::CurrentDir { message, .. }
            | ChatCommandOutcome::DirCreated { message, .. }
            | ChatCommandOutcome::Started { message, .. } => {
                self.send_text(chat, &message).await?;
            }
            ChatCommandOutcome::NoOp => {}
            ChatCommandOutcome::SelectAgent { current, options } => {
                let mut lines = vec![
                    crate::t!("qq.choose_agent").to_string(),
                    crate::t!("qq.use_agents_hint").to_string(),
                ];
                for (id, name) in options {
                    let mark = if id == current.to_string() { " *" } else { "" };
                    lines.push(format!("- {}{}", name, mark));
                }
                self.send_text(chat, &lines.join("\n")).await?;
            }
            ChatCommandOutcome::SelectModel {
                provider,
                current,
                options,
            } => {
                let mut lines = vec![crate::t_fmt!(
                    "telegram.choose_model",
                    NAME = crate::command::agents::provider_display_name(&provider)
                )];
                lines.push(crate::command::models::current_model_line(
                    current.as_deref(),
                ));
                for (i, m) in options.iter().enumerate() {
                    lines.push(crate::command::models::format_model_list_entry(
                        i,
                        m,
                        current.as_deref() == Some(m.as_str()),
                    ));
                }
                lines.push(crate::t!("models.switch_hint_raw").to_string());
                self.send_text(chat, &lines.join("\n")).await?;
            }
            ChatCommandOutcome::ListDir { dir, dirs } => {
                if dirs.is_empty() {
                    self.send_text(chat, crate::t!("builtin.no_subdirs"))
                        .await?;
                } else {
                    let listing: Vec<String> = dirs
                        .iter()
                        .map(|(name, path)| format!("{}/ → {}", name, path))
                        .collect();
                    let body = format!(
                        "{}\n{}",
                        crate::t_fmt!("qq.choose_directory", DIR = dir),
                        listing.join("\n")
                    );
                    self.send_text(chat, &body).await?;
                }
            }
            ChatCommandOutcome::History { sessions } => {
                self.send_text(chat, &outcome_text::format_history(&sessions))
                    .await?;
            }
            ChatCommandOutcome::ForwardToAgent { active, text } => {
                let _guard = runtime.poll_lock.lock().await;
                let sink = QqEventSink {
                    platform: self,
                    chat: chat.clone(),
                    channel_id: channel_id.to_string(),
                };
                let mut sink = crate::runtime::event_poller::BufferedSink::new(
                    sink,
                    Duration::from_millis(QQ_FLUSH_INTERVAL_MS),
                    QQ_MAX_BUFFER_CHARS,
                );
                GLOBAL_CHANNEL_SESSIONS
                    .send_and_poll_active_runtime_buffered(&active, &text, &mut sink)
                    .await?;
            }
        }
        Ok(())
    }

    async fn shutdown_all_sessions(&self) {
        let targets: Vec<(String, QqChatTarget)> = self
            .channels
            .iter()
            .filter_map(|e| {
                e.value()
                    .shutdown_notice_target()
                    .map(|chat| (e.key().clone(), chat))
            })
            .collect();
        for (channel_id, chat) in targets {
            if let Some(mut entry) = self.channels.get_mut(&channel_id) {
                if let Some(active) = entry.active_agent.take() {
                    let _ = active.controller.lock().await.stop_session().await;
                }
            }
            let _ = self.send_text(&chat, crate::t!("qq.shutdown_notice")).await;
        }
        self.channels.clear();
    }
}

async fn send_text_with_api(api: &QqApiClient, chat: &QqChatTarget, text: &str) -> Result<()> {
    match chat {
        QqChatTarget::C2c { openid } => api.send_c2c_text(openid, text).await,
        QqChatTarget::Group { group_openid } => api.send_group_text(group_openid, text).await,
    }
}

fn qq_file_chat_target(chat: &QqChatTarget) -> QqFileChatTarget {
    match chat {
        QqChatTarget::C2c { openid } => QqFileChatTarget::C2c {
            openid: openid.clone(),
        },
        QqChatTarget::Group { group_openid } => QqFileChatTarget::Group {
            group_openid: group_openid.clone(),
        },
    }
}

fn parse_channel_id(channel_id: &str) -> Option<(QqChatTarget, String)> {
    let file = QqFileChatTarget::from_channel_id(channel_id)?;
    let chat = qq_chat_target_from_file(&file);
    Some((chat, channel_id.to_string()))
}

fn qq_chat_target_from_file(ft: &QqFileChatTarget) -> QqChatTarget {
    match ft {
        QqFileChatTarget::C2c { openid } => QqChatTarget::C2c {
            openid: openid.clone(),
        },
        QqFileChatTarget::Group { group_openid } => QqChatTarget::Group {
            group_openid: group_openid.clone(),
        },
    }
}

#[async_trait::async_trait]
impl Platform for QqPlatform {
    async fn run(&self) -> Result<()> {
        info!(
            "[QQ] Starting platform (app_id={}, sandbox={})",
            self.config.app_id, self.config.sandbox
        );
        crate::platform::status::set_state(
            "qq",
            crate::platform::status::ConnectionState::Connecting,
        );
        self.spawn_deliver_listener();

        let (gw_tx, mut gw_rx) = mpsc::unbounded_channel();
        let api = self.api.clone();
        let gateway_handle = tokio::spawn(async move {
            if let Err(e) = ws::run_gateway(api, gw_tx).await {
                error!("[QQ] Gateway task exited: {}", e);
            }
        });

        let platform = self.clone();
        loop {
            match gw_rx.recv().await {
                Some(ws::GatewayEvent::Ready(_)) => {
                    crate::platform::status::set_state(
                        "qq",
                        crate::platform::status::ConnectionState::Connected,
                    );
                    info!("[QQ] Gateway connected");
                }
                Some(ws::GatewayEvent::Dispatch {
                    event_type, data, ..
                }) => {
                    crate::platform::status::set_state(
                        "qq",
                        crate::platform::status::ConnectionState::Connected,
                    );
                    let platform = platform.clone();
                    tokio::spawn(async move {
                        if let Err(e) = platform.handle_dispatch(&event_type, &data).await {
                            error!("[QQ] Failed to handle {}: {}", event_type, e);
                        }
                    });
                }
                Some(ws::GatewayEvent::ReconnectRequested)
                | Some(ws::GatewayEvent::InvalidSession) => {
                    crate::platform::status::set_state(
                        "qq",
                        crate::platform::status::ConnectionState::Connecting,
                    );
                }
                None => {
                    warn!("[QQ] Gateway event channel closed");
                    break;
                }
            }
        }

        gateway_handle.abort();
        Ok(())
    }

    async fn shutdown(&self) {
        self.shutdown_all_sessions().await;
    }

    fn clone_for_run(&self) -> Box<dyn Platform> {
        Box::new(self.clone())
    }
}
