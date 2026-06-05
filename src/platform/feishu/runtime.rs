//! Feishu platform runtime: payload dispatch, normalization, and card actions.

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::command::router::CommandRouter;
use crate::platform::feishu::{extract_post_content, FeishuPlatform, NormalizedMessage};
use crate::session::agent_history::{
    AgentHistoryAction, AgentHistoryEnv, AgentHistoryOutcome, AgentHistoryRequest,
    AgentHistoryStartKind,
};
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::chat_flow;

// ---------------------------------------------------------------------------
// Output buffering policy (Feishu)
// ---------------------------------------------------------------------------

const FEISHU_FLUSH_INTERVAL_MS: u64 = 200;
const FEISHU_MAX_BUFFER_CHARS: usize = 2000;

impl FeishuPlatform {
    /// Dispatch a decoded protobuf data-frame payload (JSON bytes).
    ///
    /// The WS layer ACKs and then hands payload bytes here.
    pub(crate) async fn dispatch_ws_data_payload(&self, payload: Vec<u8>) {
        let value: serde_json::Value = match serde_json::from_slice(&payload) {
            Ok(v) => v,
            Err(e) => {
                info!("Feishu data payload JSON parse error: {}", e);
                return;
            }
        };

        let event_type = value
            .get("header")
            .and_then(|h| h.get("event_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match event_type {
            "im.message.receive_v1" => {
                let Some(normalized) = self.normalize_im_event(&value) else {
                    return;
                };

                if self.dedup_cache.contains(&normalized.message_id) {
                    info!("Feishu message {} deduplicated", normalized.message_id);
                    return;
                }
                self.dedup_cache.insert(normalized.message_id.clone());

                let platform = self.clone();
                tokio::spawn(async move {
                    platform.process_normalized_message(normalized).await;
                });
            }
            "card.action.trigger" => {
                let platform = self.clone();
                tokio::spawn(async move {
                    platform.handle_card_action(&value).await;
                });
            }
            _ => {
                debug!("Feishu skip event_type: {}", event_type);
            }
        }
    }

    pub(crate) fn normalize_im_event(
        &self,
        value: &serde_json::Value,
    ) -> Option<NormalizedMessage> {
        let header = value.get("header")?;
        let event_type = header.get("event_type")?.as_str()?;
        if event_type != "im.message.receive_v1" {
            debug!("Feishu normalize_im_event: skip event_type {}", event_type);
            return None;
        }

        let event = value.get("event")?;
        let sender = event.get("sender")?;
        let sender_id = sender.get("sender_id")?;
        let sender_open_id = sender_id
            .get("open_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let message = event.get("message")?;
        let message_id = message
            .get("message_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let message_type = message
            .get("message_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();
        let chat_id = message
            .get("chat_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let chat_type = message
            .get("chat_type")
            .and_then(|v| v.as_str())
            .unwrap_or("p2p")
            .to_string();
        let content_raw = message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let (content, _image_keys) = extract_post_content(&content_raw);

        let receive_id_type = if chat_type == "p2p" { "open_id" } else { "chat_id" };
        let receive_id = if chat_type == "p2p" {
            sender_open_id.clone()
        } else {
            chat_id.clone()
        };

        use crate::platform::feishu::MentionInfo;
        let mentions = message
            .get("mentions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|m| {
                        let open_id = m
                            .get("id")
                            .and_then(|id| id.get("open_id"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = m
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        let key = m.get("key").and_then(|v| v.as_str()).map(|s| s.to_string());
                        MentionInfo { open_id, name, key }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(NormalizedMessage {
            message_id,
            message_type,
            content,
            sender_open_id,
            sender_name: None,
            chat_id: Some(chat_id.clone()),
            chat_type: Some(chat_type),
            mentions,
            raw: value.clone(),
            receive_id_type: receive_id_type.to_string(),
            receive_id: receive_id.to_string(),
        })
    }

    /// Route a normalized IM message: dispatch to Claude or handle built-in commands.
    pub async fn process_normalized_message(&self, msg: NormalizedMessage) {
        let chat_id = msg.chat_id.clone().unwrap_or_default();
        let receive_id_type = msg.receive_id_type.clone();
        let receive_id = msg.receive_id.clone();
        let message_id = msg.message_id.clone();

        // Pairing authentication check
        use crate::session::pairing::GLOBAL_PAIRING_MANAGER;
        let approved = GLOBAL_PAIRING_MANAGER.is_approved("feishu", &chat_id);
        if !approved {
            if GLOBAL_PAIRING_MANAGER.require_pairing("feishu") {
                let code = GLOBAL_PAIRING_MANAGER.get_or_create_pending("feishu", &chat_id);
                let msg_text = crate::t_fmt!("pairing.wait_message", CODE = code);
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &msg_text)
                    .await;
            }
            // When require_pairing is false: silently ignore unapproved chats.
            self.on_processing_complete(&message_id, true).await;
            return;
        }

        let content = match self.resolve_inbound_content(&msg).await {
            Ok(content) => content,
            Err(e) => {
                warn!("Feishu failed to resolve inbound media: {}", e);
                let _ = self
                    .send_text_message(
                        &receive_id_type,
                        &receive_id,
                        &crate::t_fmt!("feishu.error_generic", ERR = e),
                    )
                    .await;
                self.on_processing_complete(&message_id, false).await;
                return;
            }
        };

        if content.trim().is_empty() {
            warn!(
                "Feishu message {} type {} has no text or downloadable media",
                message_id, msg.message_type
            );
            self.on_processing_complete(&message_id, true).await;
            return;
        }

        info!(
            "Feishu processing message in chat {} content: {}",
            chat_id, content
        );

        crate::web::state::broadcast_event(&chat_id, "feishu", &chat_id, "user", &content);

        // Add typing reaction
        self.on_processing_start(&message_id).await;

        let runtime = self
            .get_channel(&chat_id, &receive_id_type, &receive_id)
            .await;

        // Build router
        let router = if let Some(ref active) = runtime.active_agent {
            CommandRouter::new(active.controller.clone(), &self.default_dir)
        } else {
            let dummy = std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::runtime::controller::AgentController::new(
                    self.agent_settings.clone(),
                    self.show_thinking
                        .load(std::sync::atomic::Ordering::Relaxed),
                ),
            ));
            {
                let ctrl = dummy.lock().await;
                let channel_wd = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                    .get_channel(&runtime.channel_session.id)
                    .map(|c| c.work_dir)
                    .unwrap_or_else(|| shellexpand::tilde(&self.default_dir).to_string());
                ctrl.init_work_dir(channel_wd).await;
            }
            CommandRouter::new(dummy, &self.default_dir)
        };

        let mcp_ctx = self.mcp_context_for_receive(&receive_id, &receive_id_type);
        let channel_work_dir = GLOBAL_CHANNEL_SESSIONS
            .get_channel(&runtime.channel_session.id)
            .map(|c| c.work_dir)
            .unwrap_or_else(|| runtime.channel_session.work_dir.clone());
        let mut context = ChatCommandContext::new(
            runtime.channel_session.id.clone(),
            format!("Feishu {}", chat_id),
            channel_work_dir,
            runtime.active_agent.clone(),
        )
        .with_mcp_context(mcp_ctx);
        let executor = ChatCommandExecutor::new(
            &self.default_dir,
            self.agent_settings.clone(),
            self.show_thinking
                .load(std::sync::atomic::Ordering::Relaxed),
        );
        let outcome =
            match chat_flow::route_and_execute(&router, &executor, &mut context, &content).await {
                Ok(outcome) => outcome,
                Err(e) => {
                    let _ = self
                        .send_text_message(
                            &receive_id_type,
                            &receive_id,
                            &crate::t_fmt!("feishu.error_generic", ERR = e),
                        )
                        .await;
                    self.on_processing_complete(&message_id, false).await;
                    return;
                }
            };

        if let Some(mut entry) = self.channels.get_mut(&chat_id) {
            entry.active_agent = context.active_agent.clone();
            entry.set_work_dir(context.channel_work_dir.clone());
        }

        match outcome {
            ChatCommandOutcome::Reply(text)
            | ChatCommandOutcome::Stopped { message: text }
            | ChatCommandOutcome::ThinkingShown { message: text }
            | ChatCommandOutcome::ThinkingHidden { message: text } => {
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &text)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::WorkDirChanged { work_dir, message }
            | ChatCommandOutcome::CurrentDir { work_dir, message } => {
                let _ = work_dir;
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &message)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::DirCreated { path, message } => {
                let _ = path;
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &message)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::Error(text) => {
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &text)
                    .await;
                self.on_processing_complete(&message_id, false).await;
            }
            ChatCommandOutcome::NoOp => {
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::SelectAgent { current, options } => {
                let card = crate::platform::feishu::cards::build_agent_picker_card(
                    &options,
                    &current,
                    &chat_id,
                    &receive_id_type,
                    &receive_id,
                );
                let _ = self
                    .send_interactive_card(&receive_id_type, &receive_id, &card)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::SelectModel {
                provider,
                current,
                options,
            } => {
                let provider_name = crate::command::agents::provider_display_name(&provider);
                let card = crate::platform::feishu::cards::build_model_picker_card(
                    &options,
                    provider_name,
                    &chat_id,
                    current.as_deref(),
                );
                let _ = self
                    .send_interactive_card(&receive_id_type, &receive_id, &card)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::ListDir { dir, dirs } => {
                if dirs.is_empty() {
                    let _ = self
                        .send_text_message(
                            &receive_id_type,
                            &receive_id,
                            crate::t!("builtin.no_subdirs"),
                        )
                        .await;
                } else {
                    let card = crate::platform::feishu::cards::build_dir_picker_card(
                        &dirs,
                        0,
                        &dir,
                        &chat_id,
                        &receive_id_type,
                        &receive_id,
                    );
                    let _ = self
                        .send_interactive_card(&receive_id_type, &receive_id, &card)
                        .await;
                }
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::Started { message, .. } => {
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &message)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::History { sessions } => {
                if sessions.is_empty() {
                    let _ = self
                        .send_text_message(
                            &receive_id_type,
                            &receive_id,
                            crate::t!("feishu.no_sessions"),
                        )
                        .await;
                } else {
                    let card = crate::platform::feishu::cards::build_session_history_card(
                        &sessions,
                        &chat_id,
                        &receive_id_type,
                        &receive_id,
                    );
                    let _ = self
                        .send_interactive_card(&receive_id_type, &receive_id, &card)
                        .await;
                }
                self.on_processing_complete(&message_id, true).await;
            }
            ChatCommandOutcome::ForwardToAgent { active, text } => {
                let _guard = runtime.poll_lock.lock().await;
                struct FeishuEventSink<'a> {
                    platform: &'a FeishuPlatform,
                    receive_id_type: String,
                    receive_id: String,
                    chat_id_str: String,
                    sender_open_id: String,
                }

                #[async_trait::async_trait]
                impl<'a> crate::runtime::event_poller::EventPollSink for FeishuEventSink<'a> {
                    async fn flush(&mut self, text: &str, is_done: bool) -> Result<()> {
                        let _ = is_done;
                        if text.trim().is_empty() {
                            return Ok(());
                        }
                        self.platform
                            .send_text_message(&self.receive_id_type, &self.receive_id, text)
                            .await?;
                        crate::web::state::broadcast_event(
                            &self.chat_id_str,
                            "feishu",
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
                        input: Option<&serde_json::Value>,
                    ) -> Result<()> {
                        self.platform.pending_permissions.insert(
                            request_id.to_string(),
                            crate::platform::feishu::PendingPermissionContext {
                                request_id: request_id.to_string(),
                                tool_name: tool_name.to_string(),
                                chat_id: self.chat_id_str.clone(),
                                sender_open_id: self.sender_open_id.clone(),
                                input: input.cloned(),
                                created_at: std::time::Instant::now(),
                            },
                        );
                        let card = crate::platform::feishu::cards::build_permission_card(
                            request_id,
                            tool_name,
                            &self.chat_id_str,
                        );
                        self.platform
                            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
                            .await?;
                        crate::web::state::broadcast_event(
                            &self.chat_id_str,
                            "feishu",
                            &self.chat_id_str,
                            "system",
                            &crate::t_fmt!(
                                "feishu.permission_request_text",
                                NAME = tool_name,
                                ID = request_id
                            ),
                        );
                        Ok(())
                    }

                    async fn on_confirm_request(
                        &mut self,
                        request_id: &str,
                        prompt: &str,
                        options: &[String],
                    ) -> Result<()> {
                        let card = crate::platform::feishu::cards::build_select_card(
                            request_id,
                            prompt,
                            options,
                            &self.chat_id_str,
                        );
                        self.platform
                            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
                            .await?;
                        Ok(())
                    }

                    async fn on_select_request(
                        &mut self,
                        request_id: &str,
                        prompt: &str,
                        options: &[String],
                    ) -> Result<()> {
                        let card = crate::platform::feishu::cards::build_select_card(
                            request_id,
                            prompt,
                            options,
                            &self.chat_id_str,
                        );
                        self.platform
                            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
                            .await?;
                        Ok(())
                    }

                    async fn on_question_request(
                        &mut self,
                        _request_id: &str,
                        _questions: &[crate::runtime::controller::QuestionItem],
                    ) -> Result<()> {
                        Ok(())
                    }
                }

                let sink = FeishuEventSink {
                    platform: self,
                    receive_id_type: receive_id_type.clone(),
                    receive_id: receive_id.clone(),
                    chat_id_str: chat_id.clone(),
                    sender_open_id: msg.sender_open_id.clone(),
                };
                // Feishu: time-first flush (200ms), buffer max 2000 chars.
                let mut sink = crate::runtime::event_poller::BufferedSink::new(
                    sink,
                    std::time::Duration::from_millis(FEISHU_FLUSH_INTERVAL_MS),
                    FEISHU_MAX_BUFFER_CHARS,
                );

                match GLOBAL_CHANNEL_SESSIONS
                    .send_and_poll_active_runtime_buffered(&active, &text, &mut sink)
                    .await
                {
                    Ok(()) => {
                        self.on_processing_complete(&message_id, true).await;
                    }
                    Err(e) => {
                        let _ = self
                            .send_text_message(
                                &receive_id_type,
                                &receive_id,
                                &crate::t_fmt!("feishu.error_generic", ERR = e),
                            )
                            .await;
                        self.on_processing_complete(&message_id, false).await;
                    }
                }
            }
        }
    }

    pub(crate) async fn handle_card_action(&self, value: &serde_json::Value) {
        let header = match value.get("header") {
            Some(h) => h,
            None => return,
        };
        let event_type = match header.get("event_type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => return,
        };

        if event_type != "card.action.trigger" {
            return;
        }

        let event = match value.get("event") {
            Some(e) => e,
            None => return,
        };

        let action_obj = match event.get("action") {
            Some(a) => a,
            None => return,
        };

        // Feishu may nest the value under action.value, or directly in action
        let user_value = action_obj.get("value").unwrap_or(action_obj);

        let cmd = user_value.get("cmd").and_then(|v| v.as_str());
        let chat_id = user_value
            .get("chat_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let receive_id_type = match user_value.get("receive_id_type").and_then(|v| v.as_str()) {
            Some(t) => t.to_string(),
            None => self
                .channels
                .get(chat_id)
                .map(|r| r.receive_id_type.clone())
                .unwrap_or_else(|| "chat_id".to_string()),
        };
        if chat_id.is_empty() {
            return;
        }

        let receive_id = user_value
            .get("receive_id")
            .and_then(|v| v.as_str())
            .unwrap_or(chat_id)
            .to_string();

        let open_message_id = event
            .get("context")
            .and_then(|c| c.get("open_message_id"))
            .and_then(|v| v.as_str());

        // Immediately disable all buttons on the card to prevent double-clicks.
        // Skip pagination (ll_page) which just refreshes the same card.
        if let (Some(mid), Some(cmd)) = (open_message_id, cmd) {
            if cmd != "ll_page" {
                if let Some(original) = self.sent_cards.get(mid) {
                    let disabled = crate::platform::feishu::cards::disable_card_buttons(&original);
                    let _ = self.update_interactive_card(mid, &disabled).await;
                }
            }
        }

        match cmd {
            // --- /ll page navigation ---
            Some("ll_page") => {
                let page = user_value.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                let dir = user_value.get("dir").and_then(|v| v.as_str()).unwrap_or("");
                if !dir.is_empty() {
                    let dir = match crate::command::workdir::resolve_work_dir_target(
                        "",
                        &self.default_dir,
                        std::path::Path::new(dir),
                    ) {
                        Ok(dir) => dir,
                        Err(e) => {
                            let _ = self
                                .send_text_message(&receive_id_type, &receive_id, &e.to_string())
                                .await;
                            return;
                        }
                    };
                    if let Ok(dirs) = crate::command::builtin::list_directory_paths(&dir) {
                        if !dirs.is_empty() {
                            let card_dirs: Vec<(String, String)> = dirs
                                .iter()
                                .map(|(name, path)| (name.clone(), path.clone()))
                                .collect();
                            let card = crate::platform::feishu::cards::build_dir_picker_card(
                                &card_dirs,
                                page,
                                &dir,
                                chat_id,
                                &receive_id_type,
                                &receive_id,
                            );
                            if let Some(mid) = open_message_id {
                                let _ = self.update_interactive_card(mid, &card).await;
                            } else {
                                let _ = self
                                    .send_interactive_card(&receive_id_type, &receive_id, &card)
                                    .await;
                            }
                        }
                    }
                }
            }

            Some("set_agent") => {
                let provider = user_value
                    .get("provider")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if provider.is_empty() {
                    return;
                }
                let runtime = self
                    .get_channel(chat_id, &receive_id_type, &receive_id)
                    .await;
                let provider = crate::config::model::AgentProvider::parse_str(provider);
                let name = crate::command::agents::provider_display_name(&provider);
                match GLOBAL_CHANNEL_SESSIONS
                    .set_channel_default_provider(&runtime.channel_session.id, provider)
                {
                    Ok(()) => {
                        let title = crate::t!("feishu.card_agent_set_title");
                        let text = crate::t_fmt!("builtin.channel_agent_set", NAME = name);
                        let card =
                            crate::platform::feishu::cards::build_result_card(title, &text, true);
                        if let Some(mid) = open_message_id {
                            let _ = self.update_interactive_card(mid, &card).await;
                        } else {
                            let _ = self
                                .send_text_message(&receive_id_type, &receive_id, &text)
                                .await;
                        }
                    }
                    Err(e) => {
                        let title = crate::t!("feishu.card_agent_set_title");
                        let err_text = crate::t_fmt!("builtin.failed_set_channel_agent", ERR = e);
                        let card = crate::platform::feishu::cards::build_result_card(
                            title, &err_text, false,
                        );
                        if let Some(mid) = open_message_id {
                            let _ = self.update_interactive_card(mid, &card).await;
                        } else {
                            let _ = self
                                .send_text_message(&receive_id_type, &receive_id, &err_text)
                                .await;
                        }
                    }
                }
            }

            Some("set_model") => {
                let model_id = user_value
                    .get("model_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if model_id.trim().is_empty() {
                    return;
                }
                let runtime = self
                    .get_channel(chat_id, &receive_id_type, &receive_id)
                    .await;
                if let Some(ref active) = runtime.active_agent {
                    let ctrl = active.controller.lock().await;
                    let provider =
                        crate::config::model::AgentProvider::parse_str(&ctrl.provider_name().await);
                    let result = ctrl.switch_model(&model_id).await;
                    let title = crate::t!("feishu.model_picker_title");
                    let card = match result {
                        Ok(canonical) => {
                            let text = crate::t_fmt!(
                                "models.switched",
                                NAME = crate::command::agents::provider_display_name(&provider),
                                MODEL = canonical
                            );
                            crate::platform::feishu::cards::build_result_card(title, &text, true)
                        }
                        Err(e) => {
                            let text = crate::t_fmt!("models.switch_failed", ERR = e);
                            crate::platform::feishu::cards::build_result_card(title, &text, false)
                        }
                    };
                    if let Some(mid) = open_message_id {
                        let _ = self.update_interactive_card(mid, &card).await;
                    } else {
                        let _ = self
                            .send_interactive_card(&receive_id_type, &receive_id, &card)
                            .await;
                    }
                }
            }

            // --- /ll directory select ---
            Some("cd") => {
                if let Some(path) = user_value.get("path").and_then(|v| v.as_str()) {
                    let runtime = self
                        .get_channel(chat_id, &receive_id_type, &receive_id)
                        .await;
                    let current_dir = if let Some(ref active) = runtime.active_agent {
                        let ctrl = active.controller.lock().await;
                        crate::command::workdir::effective_work_dir(
                            &ctrl.get_work_dir().await,
                            &self.default_dir,
                        )
                    } else {
                        runtime.channel_session.work_dir.clone()
                    };
                    match crate::command::workdir::resolve_work_dir_target(
                        &current_dir,
                        &self.default_dir,
                        std::path::Path::new(path),
                    ) {
                        Ok(path_str) => {
                            if let Some(ref active) = runtime.active_agent {
                                let ctrl = active.controller.lock().await;
                                ctrl.init_work_dir(path_str.clone()).await;
                            }
                            let _ = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                                .switch_work_dir(
                                    &runtime.channel_session.id,
                                    std::path::PathBuf::from(&path_str),
                                )
                                .await;
                            if let Some(mut entry) = self.channels.get_mut(chat_id) {
                                entry.set_work_dir(path_str.clone());
                            }
                            let title = crate::t!("feishu.card_dir_changed_title");
                            let text = crate::t_fmt!("feishu.dir_changed", PATH = path_str);
                            let card = crate::platform::feishu::cards::build_result_card(
                                title, &text, true,
                            );
                            if let Some(mid) = open_message_id {
                                let _ = self.update_interactive_card(mid, &card).await;
                            } else {
                                let _ = self
                                    .send_text_message(&receive_id_type, &receive_id, &text)
                                    .await;
                            }
                        }
                        Err(e) => {
                            let err_text = e.to_string();
                            let title = crate::t!("feishu.card_dir_changed_title");
                            let card = crate::platform::feishu::cards::build_result_card(
                                title, &err_text, false,
                            );
                            if let Some(mid) = open_message_id {
                                let _ = self.update_interactive_card(mid, &card).await;
                            } else {
                                let _ = self
                                    .send_text_message(&receive_id_type, &receive_id, &err_text)
                                    .await;
                            }
                        }
                    }
                }
            }

            // --- agent-history card: resume / new session / delete ---
            Some("resume") | Some("delete_session") => {
                let session_id = user_value
                    .get("session_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let action = match cmd {
                    Some("delete_session") => AgentHistoryAction::Delete { session_id },
                    Some("resume") if session_id.is_empty() => {
                        let work_dir = user_value
                            .get("work_dir")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&self.default_dir)
                            .to_string();
                        AgentHistoryAction::StartNew { work_dir }
                    }
                    Some("resume") => AgentHistoryAction::Resume { session_id },
                    _ => return,
                };
                self.handle_agent_history_card_action(
                    chat_id,
                    &receive_id_type,
                    &receive_id,
                    open_message_id.as_deref(),
                    action,
                )
                .await;
            }

            // --- Permission card: allow / deny ---
            Some("allow") | Some("deny") => {
                let request_id = user_value
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let is_allow = cmd == Some("allow");
                if !request_id.is_empty() {
                    let stored_input = self
                        .pending_permissions
                        .remove(request_id)
                        .and_then(|(_, ctx)| ctx.input);
                    let runtime = self
                        .get_channel(chat_id, &receive_id_type, &receive_id)
                        .await;
                    if let Some(ref active) = runtime.active_agent {
                        let ctrl = active.controller.lock().await;
                        if is_allow {
                            let allow_msg = crate::runtime::protocol::build_permission_allow(
                                request_id,
                                stored_input,
                            );
                            let _ = ctrl.send_input(allow_msg).await;
                        } else {
                            let deny_msg = crate::runtime::protocol::build_permission_deny(
                                request_id,
                                "User denied permission",
                            );
                            let _ = ctrl.send_input(deny_msg).await;
                        }
                    }
                    // Update the permission card to show result
                    let title = crate::t!("feishu.permission_title");
                    let text = if is_allow {
                        crate::t!("feishu.card_allowed")
                    } else {
                        crate::t!("feishu.card_denied")
                    };
                    let card =
                        crate::platform::feishu::cards::build_result_card(title, text, is_allow);
                    if let Some(mid) = open_message_id {
                        let _ = self.update_interactive_card(mid, &card).await;
                    }
                }
            }

            // --- Select card: choose option ---
            Some("select") => {
                let request_id = user_value
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !request_id.is_empty() {
                    let runtime = self
                        .get_channel(chat_id, &receive_id_type, &receive_id)
                        .await;
                    if let Some(ref active) = runtime.active_agent {
                        let ctrl = active.controller.lock().await;
                        let msg =
                            crate::runtime::protocol::build_permission_allow(request_id, None);
                        let _ = ctrl.send_input(msg).await;
                    }
                    // Update the select card to show result
                    let title = crate::t!("feishu.card_selected_title");
                    let text = crate::t!("feishu.card_selected");
                    let card = crate::platform::feishu::cards::build_result_card(title, text, true);
                    if let Some(mid) = open_message_id {
                        let _ = self.update_interactive_card(mid, &card).await;
                    }
                }
            }

            _ => {
                debug!("Unhandled card action: {:?}", user_value);
            }
        }
    }

    fn agent_history_env(&self) -> AgentHistoryEnv {
        AgentHistoryEnv {
            default_dir: self.default_dir.clone(),
            agent_settings: self.agent_settings.clone(),
            show_thinking: self
                .show_thinking
                .load(std::sync::atomic::Ordering::Relaxed),
        }
    }

    async fn handle_agent_history_card_action(
        &self,
        chat_id: &str,
        receive_id_type: &str,
        receive_id: &str,
        open_message_id: Option<&str>,
        action: AgentHistoryAction,
    ) {
        let runtime = self
            .get_channel(chat_id, receive_id_type, receive_id)
            .await;
        let req = AgentHistoryRequest {
            channel_id: runtime.channel_session.id.clone(),
            title: format!("Feishu {}", chat_id),
            mcp_context: Some(self.mcp_context_for_receive(receive_id, receive_id_type)),
        };
        let action_for_deliver = action.clone();
        let outcome =
            crate::session::agent_history::run(&self.agent_history_env(), &req, action).await;
        if let AgentHistoryOutcome::Started { ref active, .. } = outcome {
            if let Some(mut entry) = self.channels.get_mut(chat_id) {
                entry.active_agent = Some(active.clone());
                entry.set_work_dir(active.agent_session.work_dir.clone());
            }
        }
        self.deliver_agent_history_outcome(
            &action_for_deliver,
            outcome,
            open_message_id,
            receive_id_type,
            receive_id,
        )
        .await;
    }

    async fn deliver_agent_history_outcome(
        &self,
        action: &AgentHistoryAction,
        outcome: AgentHistoryOutcome,
        open_message_id: Option<&str>,
        receive_id_type: &str,
        receive_id: &str,
    ) {
        let (title, text, success) = match outcome {
            AgentHistoryOutcome::Started { message, kind, .. } => {
                let title = match kind {
                    AgentHistoryStartKind::New => crate::t!("feishu.card_started_title"),
                    AgentHistoryStartKind::Resumed => crate::t!("feishu.card_resumed_title"),
                };
                (title, message, true)
            }
            AgentHistoryOutcome::Deleted { message, success } => {
                (crate::t!("feishu.card_session_deleted_title"), message, success)
            }
            AgentHistoryOutcome::Error { message } => {
                let title = match action {
                    AgentHistoryAction::StartNew { .. } => {
                        crate::t!("feishu.card_started_title")
                    }
                    AgentHistoryAction::Delete { .. } => {
                        crate::t!("feishu.card_session_deleted_title")
                    }
                    _ => crate::t!("feishu.card_resumed_title"),
                };
                (title, message, false)
            }
            AgentHistoryOutcome::List { .. } => return,
        };
        let card = crate::platform::feishu::cards::build_result_card(title, &text, success);
        if let Some(mid) = open_message_id {
            let _ = self.update_interactive_card(mid, &card).await;
        } else {
            let _ = self
                .send_text_message(receive_id_type, receive_id, &text)
                .await;
        }
    }
}
