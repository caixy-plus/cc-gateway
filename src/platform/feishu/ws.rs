use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::time::{sleep, Duration as TokioDuration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tracing::{debug, error, info, warn};

use crate::claude::event_poller::ClaudeEventPoller;
use crate::command::router::CommandRouter;
use crate::platform::feishu::{
    build_ack_frame, build_ping_frame, extract_post_content, FeishuPlatform, NormalizedMessage,
    METHOD_CONTROL, METHOD_DATA,
};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

use super::WsClientConfig;

impl FeishuPlatform {
    pub async fn run_websocket(&self, ws_url: &str, client_config: WsClientConfig) -> Result<()> {
        info!("Connecting to Feishu WebSocket: {}", ws_url);

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to Feishu WebSocket")?;
        info!("Feishu WebSocket connected successfully");

        let (write, mut read) = ws_stream.split();
        let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));

        // Extract service_id from URL (matching main branch behavior)
        let service_id = ws_url
            .split('?')
            .nth(1)
            .and_then(|q| q.split('&').find(|p| p.starts_with("service_id=")))
            .and_then(|p| p.strip_prefix("service_id="))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0i32);

        // Send initial PING immediately so Feishu server acknowledges the new connection
        {
            let ping = build_ping_frame(service_id);
            let ping_bytes = crate::platform::proto::encode_frame(&ping);
            let mut w = write.lock().await;
            if w.send(WsMessage::Binary(ping_bytes.into())).await.is_ok() {
                info!("Sent initial PING for service_id={}", service_id);
            }
        }

        // Small delay to stabilize the connection before entering the read loop
        sleep(TokioDuration::from_millis(200)).await;

        // Spawn heartbeat
        let heartbeat_write = write.clone();
        let heartbeat_handle = tokio::spawn(async move {
            let interval = TokioDuration::from_secs(client_config.ping_interval.max(1) as u64);
            loop {
                sleep(interval).await;
                let ping = build_ping_frame(service_id);
                let ping_bytes = crate::platform::proto::encode_frame(&ping);
                let mut w = heartbeat_write.lock().await;
                if let Err(e) = w.send(WsMessage::Binary(ping_bytes.into())).await {
                    warn!("Feishu WebSocket heartbeat send error: {}", e);
                    break;
                }
            }
        });

        // Read loop (matching main branch per-message decode pattern)
        let platform = self.clone();
        let read_timeout_secs = client_config.ping_interval.max(1) as u64 * 3;
        let read_result: Result<()> = async {
            loop {
                match tokio::time::timeout(TokioDuration::from_secs(read_timeout_secs), read.next())
                    .await
                {
                    Ok(Some(Ok(WsMessage::Binary(data)))) => {
                        // Decode directly without buffering (matching main branch behavior)
                        if let Some(frame) = crate::platform::proto::Frame::decode(&data) {
                            platform.handle_frame(&frame, &write).await;
                        } else {
                            info!("Received invalid protobuf frame ({} bytes)", data.len());
                        }
                    }
                    Ok(Some(Ok(WsMessage::Ping(data)))) => {
                        let mut w = write.lock().await;
                        w.send(WsMessage::Pong(data)).await.ok();
                    }
                    Ok(Some(Ok(WsMessage::Close(_)))) => {
                        info!("Feishu WebSocket connection closed by server");
                        break;
                    }
                    Ok(Some(Ok(WsMessage::Text(text)))) => {
                        info!("Unexpected text frame: {}", text);
                    }
                    Ok(Some(Ok(_))) => {}
                    Ok(Some(Err(e))) => {
                        error!("Feishu WebSocket read error: {}", e);
                        return Err(anyhow::anyhow!("Feishu WebSocket read error: {}", e));
                    }
                    Ok(None) => {
                        info!("Feishu WebSocket stream ended");
                        break;
                    }
                    Err(_) => {
                        warn!("Feishu WebSocket read timeout after {}s", read_timeout_secs);
                        return Err(anyhow::anyhow!("Feishu WebSocket read timeout"));
                    }
                }
            }
            info!("Feishu WebSocket read loop ended");
            Ok(())
        }
        .await;

        heartbeat_handle.abort();
        read_result
    }

    async fn handle_frame(
        &self,
        frame: &crate::platform::proto::Frame,
        write: &std::sync::Arc<
            tokio::sync::Mutex<
                futures::stream::SplitSink<
                    tokio_tungstenite::WebSocketStream<
                        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
                    >,
                    WsMessage,
                >,
            >,
        >,
    ) {
        info!(
            "Feishu frame: method={} service={} payload_len={:?} headers={:?}",
            frame.method,
            frame.service,
            frame.payload.as_ref().map(|p| p.len()),
            frame
                .headers
                .iter()
                .map(|h| format!("{}={}", h.key, h.value))
                .collect::<Vec<_>>(),
        );

        match frame.method {
            METHOD_CONTROL => {
                info!(
                    "Feishu control frame type={:?}",
                    frame
                        .headers
                        .iter()
                        .find(|h| h.key == "type")
                        .map(|h| &h.value)
                );
                // Handle pong with ClientConfig (update ping interval if applicable)
                if let Some(ref payload) = frame.payload {
                    if let Ok(cfg) = serde_json::from_slice::<serde_json::Value>(payload) {
                        info!("Feishu control frame payload: {}", cfg);
                    }
                }
            }
            METHOD_DATA => {
                // ACK immediately (matching main branch behavior)
                let ack = build_ack_frame(frame);
                let ack_bytes = crate::platform::proto::encode_frame(&ack);
                let mut w = write.lock().await;
                if let Err(e) = w.send(WsMessage::Binary(ack_bytes.into())).await {
                    warn!("Failed to send ACK: {}", e);
                }
                drop(w);

                // Try IM first, then Card fallback (matching main branch)
                self.handle_im_frame(frame).await;
                self.handle_card_frame(frame).await;
            }
            _ => {
                info!("Feishu unhandled frame method={}", frame.method);
            }
        }
    }

    async fn handle_im_frame(&self, frame: &crate::platform::proto::Frame) {
        let payload = match &frame.payload {
            Some(p) => p.clone(),
            None => {
                info!("Feishu IM frame has no payload");
                return;
            }
        };

        let value: serde_json::Value = match serde_json::from_slice(&payload) {
            Ok(v) => v,
            Err(e) => {
                info!("Feishu IM frame JSON parse error: {}", e);
                return;
            }
        };

        let event_type = value
            .get("header")
            .and_then(|h| h.get("event_type"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        info!("Feishu received event_type: {}", event_type);

        let normalized = match self.normalize_im_event(&value) {
            Some(n) => n,
            None => {
                info!(
                    "Feishu normalize_im_event returned None for event_type: {}",
                    event_type
                );
                return;
            }
        };

        info!(
            "Feishu message from chat {} sender {}: {}",
            normalized.chat_id.as_deref().unwrap_or("?"),
            normalized.sender_open_id,
            normalized.content
        );

        // Deduplicate
        if self.dedup_cache.contains(&normalized.message_id) {
            info!("Feishu message {} deduplicated", normalized.message_id);
            return;
        }
        self.dedup_cache.insert(normalized.message_id.clone());

        // Process asynchronously so the WebSocket read loop is not blocked
        let platform = self.clone();
        tokio::spawn(async move {
            platform.process_normalized_message(normalized).await;
        });
    }

    async fn handle_card_frame(&self, frame: &crate::platform::proto::Frame) {
        let payload = match &frame.payload {
            Some(p) => p.clone(),
            None => return,
        };

        let value: serde_json::Value = match serde_json::from_slice(&payload) {
            Ok(v) => v,
            Err(e) => {
                info!("Feishu card frame JSON parse error: {}", e);
                return;
            }
        };

        // Process asynchronously so the WebSocket read loop is not blocked
        let platform = self.clone();
        tokio::spawn(async move {
            platform.handle_card_action(&value).await;
        });
    }

    pub(crate) fn normalize_im_event(
        &self,
        value: &serde_json::Value,
    ) -> Option<NormalizedMessage> {
        let header = value.get("header")?;
        let event_type = header.get("event_type")?.as_str()?;
        if event_type != "im.message.receive_v1" {
            info!("Feishu normalize_im_event: skip event_type {}", event_type);
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

        // ACL check
        if !self.is_allowed_sender(&sender_open_id) {
            warn!("Feishu ACL blocked sender {}", sender_open_id);
            return None;
        }

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

        let receive_id_type = if chat_type == "p2p" {
            "open_id"
        } else {
            "chat_id"
        };
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
                    .filter_map(|m| {
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
                        Some(MentionInfo { open_id, name, key })
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

        info!(
            "Feishu processing message in chat {} content: {}",
            chat_id, msg.content
        );

        // Add typing reaction
        self.on_processing_start(&message_id).await;

        let runtime = self
            .get_channel(&chat_id, &receive_id_type, &receive_id)
            .await;

        // Build router
        let router = if let Some(ref active) = runtime.active_claude {
            CommandRouter::new(active.controller.clone(), &self.default_dir)
        } else {
            let dummy = std::sync::Arc::new(tokio::sync::Mutex::new(
                crate::claude::controller::ClaudeController::new(
                    self.claude_config.clone(),
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

        let action = router.route(&msg.content).await;

        match action {
            crate::command::CommandAction::Reply(text) => {
                let _ = self
                    .send_text_message(&receive_id_type, &receive_id, &text)
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            crate::command::CommandAction::NoOp => {
                self.on_processing_complete(&message_id, true).await;
            }
            crate::command::CommandAction::ListDir { path } => {
                let current_dir = if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    crate::command::workdir::effective_work_dir(
                        &ctrl.get_work_dir().await,
                        &self.default_dir,
                    )
                } else {
                    crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                        .get_channel(&runtime.channel_session.id)
                        .map(|c| c.work_dir)
                        .unwrap_or_else(|| shellexpand::tilde(&self.default_dir).to_string())
                };
                let requested = path.unwrap_or_else(|| std::path::PathBuf::from("."));
                let dir_str = match crate::command::workdir::resolve_work_dir_target(
                    &current_dir,
                    &self.default_dir,
                    &requested,
                ) {
                    Ok(dir) => dir,
                    Err(e) => {
                        let _ = self
                            .send_text_message(&receive_id_type, &receive_id, &e.to_string())
                            .await;
                        self.on_processing_complete(&message_id, false).await;
                        return;
                    }
                };
                match crate::command::builtin::list_directory_paths(&dir_str) {
                    Ok(dirs) => {
                        if dirs.is_empty() {
                            let _ = self
                                .send_text_message(
                                    &receive_id_type,
                                    &receive_id,
                                    crate::t!("builtin.no_subdirs"),
                                )
                                .await;
                        } else {
                            let card_dirs: Vec<(String, String)> = dirs
                                .iter()
                                .map(|(name, path)| (name.clone(), path.clone()))
                                .collect();
                            let card = crate::platform::feishu::cards::build_dir_picker_card(
                                &card_dirs,
                                0,
                                &dir_str,
                                &chat_id,
                                &receive_id_type,
                                &receive_id,
                            );
                            let _ = self
                                .send_interactive_card(&receive_id_type, &receive_id, &card)
                                .await;
                        }
                    }
                    Err(e) => {
                        let _ = self
                            .send_text_message(
                                &receive_id_type,
                                &receive_id,
                                &crate::t_fmt!("feishu.error_generic", ERR = e),
                            )
                            .await;
                    }
                }
                self.on_processing_complete(&message_id, true).await;
            }
            crate::command::CommandAction::StartSession { work_dir, args } => {
                let effective_dir = work_dir
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| {
                        let latest = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                            .get_channel(&runtime.channel_session.id)
                            .map(|c| c.work_dir)
                            .unwrap_or_default();
                        if latest.is_empty() {
                            shellexpand::tilde(&self.default_dir).to_string()
                        } else {
                            latest
                        }
                    });
                let mcp_ctx = crate::claude::mcp_server::McpContext {
                    feishu_app_id: self.config.app_id.clone(),
                    feishu_app_secret: self.config.app_secret.clone(),
                    chat_id: receive_id.clone(),
                    receive_id_type: receive_id_type.clone(),
                };
                match crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                    .start_claude_session_for_platform(
                        &runtime.channel_session.id,
                        &format!("Feishu {}", chat_id),
                        &self.default_dir,
                        self.claude_config.clone(),
                        self.show_thinking
                            .load(std::sync::atomic::Ordering::Relaxed),
                        args,
                        None,
                        None,
                        Some(mcp_ctx),
                    )
                    .await
                {
                    Ok(active) => {
                        if let Some(mut entry) = self.channels.get_mut(&chat_id) {
                            entry.active_claude = Some(active.clone());
                        }
                        let _ = self
                            .send_text_message(
                                &receive_id_type,
                                &receive_id,
                                &crate::t_fmt!("feishu.session_started", DIR = effective_dir),
                            )
                            .await;
                        self.on_processing_complete(&message_id, true).await;
                    }
                    Err(e) => {
                        let _ = self
                            .send_text_message(
                                &receive_id_type,
                                &receive_id,
                                &crate::t_fmt!("builtin.failed_start_claude", ERR = e),
                            )
                            .await;
                        self.on_processing_complete(&message_id, false).await;
                    }
                }
            }
            crate::command::CommandAction::StopSession => {
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    let _ = ctrl.stop_session().await;
                }
                let _ = GLOBAL_CHANNEL_SESSIONS
                    .stop_channel_session(&runtime.channel_session.id)
                    .await;
                if let Some(mut entry) = self.channels.get_mut(&chat_id) {
                    entry.active_claude = None;
                }
                let _ = self
                    .send_text_message(
                        &receive_id_type,
                        &receive_id,
                        crate::t!("builtin.session_stopped"),
                    )
                    .await;
                self.on_processing_complete(&message_id, true).await;
            }
            crate::command::CommandAction::ShowClaudeHistory { .. } => {
                let sessions = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                    .list_claude_sessions_by_channel(&runtime.channel_session.id, Some(10));
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
                        &receive_id_type,
                        &receive_id,
                    );
                    let _ = self
                        .send_interactive_card(&receive_id_type, &receive_id, &card)
                        .await;
                }
                self.on_processing_complete(&message_id, true).await;
            }
            crate::command::CommandAction::ChangeDir(_)
            | crate::command::CommandAction::ChangeDirDefault => {
                let response = router.execute(action).await;
                if let Some(text) = response {
                    let _ = self
                        .send_text_message(&receive_id_type, &receive_id, &text)
                        .await;
                }
                let work_dir = router.current_work_dir().await;
                let _ = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                    .switch_work_dir(
                        &runtime.channel_session.id,
                        std::path::PathBuf::from(&work_dir),
                    )
                    .await;
                if let Some(mut entry) = self.channels.get_mut(&chat_id) {
                    entry.set_work_dir(work_dir);
                }
                self.on_processing_complete(&message_id, true).await;
            }
            crate::command::CommandAction::ForwardToClaude(text) => {
                // Ensure we have an active Claude session
                let active = match runtime.active_claude {
                    Some(ref a) => {
                        let ctrl = a.controller.lock().await;
                        if ctrl.is_session_active().await {
                            drop(ctrl);
                            a.clone()
                        } else {
                            drop(ctrl);
                            // Session died, clear it and create a new one
                            if let Some(mut entry) = self.channels.get_mut(&chat_id) {
                                entry.active_claude = None;
                            }
                            let mcp_ctx = crate::claude::mcp_server::McpContext {
                                feishu_app_id: self.config.app_id.clone(),
                                feishu_app_secret: self.config.app_secret.clone(),
                                chat_id: receive_id.clone(),
                                receive_id_type: receive_id_type.clone(),
                            };
                            match crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                                .start_claude_session_for_platform(
                                    &runtime.channel_session.id,
                                    &format!("Feishu {}", chat_id),
                                    &self.default_dir,
                                    self.claude_config.clone(),
                                    self.show_thinking
                                        .load(std::sync::atomic::Ordering::Relaxed),
                                    vec![],
                                    None,
                                    None,
                                    Some(mcp_ctx),
                                )
                                .await
                            {
                                Ok(active) => {
                                    if let Some(mut entry) = self.channels.get_mut(&chat_id) {
                                        entry.active_claude = Some(active.clone());
                                    }
                                    active
                                }
                                Err(e) => {
                                    let _ = self
                                        .send_text_message(
                                            &receive_id_type,
                                            &receive_id,
                                            &crate::t_fmt!("builtin.failed_start_claude", ERR = e),
                                        )
                                        .await;
                                    self.on_processing_complete(&message_id, false).await;
                                    return;
                                }
                            }
                        }
                    }
                    None => {
                        // No active session — prompt user to start one with /claude
                        let _ = self
                            .send_text_message(
                                &receive_id_type,
                                &receive_id,
                                crate::t!("controller.no_active_session"),
                            )
                            .await;
                        self.on_processing_complete(&message_id, false).await;
                        return;
                    }
                };

                // Send the message
                {
                    let ctrl = active.controller.lock().await;
                    if let Err(e) = ctrl.send_message(&text).await {
                        let _ = self
                            .send_text_message(
                                &receive_id_type,
                                &receive_id,
                                &crate::t_fmt!("feishu.failed_send", ERR = e),
                            )
                            .await;
                        self.on_processing_complete(&message_id, false).await;
                        return;
                    }
                }

                // Poll for response
                let _guard = runtime.poll_lock.lock().await;
                struct FeishuEventSink<'a> {
                    platform: &'a FeishuPlatform,
                    receive_id_type: String,
                    receive_id: String,
                    chat_id_str: String,
                }

                #[async_trait::async_trait]
                impl<'a> crate::claude::event_poller::EventPollSink for FeishuEventSink<'a> {
                    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
                        if !text.trim().is_empty() {
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
                        }
                        Ok(())
                    }

                    async fn on_permission_request(
                        &mut self,
                        request_id: &str,
                        tool_name: &str,
                        _input: Option<&serde_json::Value>,
                    ) -> Result<()> {
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
                        _questions: &[crate::claude::controller::QuestionItem],
                    ) -> Result<()> {
                        Ok(())
                    }
                }

                let mut sink = FeishuEventSink {
                    platform: self,
                    receive_id_type,
                    receive_id,
                    chat_id_str: chat_id,
                };

                let poller = {
                    let ctrl = active.controller.lock().await;
                    ClaudeEventPoller::from_controller(&*ctrl)
                };

                match poller.run(&mut sink).await {
                    Ok(()) => {
                        self.on_processing_complete(&message_id, true).await;
                    }
                    Err(e) => {
                        let _ = self
                            .send_text_message(
                                &sink.receive_id_type,
                                &sink.receive_id,
                                &crate::t_fmt!("feishu.error_generic", ERR = e),
                            )
                            .await;
                        self.on_processing_complete(&message_id, false).await;
                    }
                }
            }
            other => {
                let response = router.execute(other).await;
                if let Some(text) = response {
                    let _ = self
                        .send_text_message(&receive_id_type, &receive_id, &text)
                        .await;
                }
                self.on_processing_complete(&message_id, true).await;
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
        } else {
            let receive_id = user_value
                .get("receive_id")
                .and_then(|v| v.as_str())
                .unwrap_or(chat_id)
                .to_string();

            match cmd {
                // --- /ll page navigation ---
                Some("ll_page") => {
                    let page =
                        user_value.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
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
                                    .send_text_message(
                                        &receive_id_type,
                                        &receive_id,
                                        &e.to_string(),
                                    )
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
                                let _ = self
                                    .send_interactive_card(&receive_id_type, &receive_id, &card)
                                    .await;
                            }
                        }
                    }
                }

                // --- /ll directory select ---
                Some("cd") => {
                    if let Some(path) = user_value.get("path").and_then(|v| v.as_str()) {
                        let runtime = self
                            .get_channel(chat_id, &receive_id_type, &receive_id)
                            .await;
                        let current_dir = if let Some(ref active) = runtime.active_claude {
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
                                if let Some(ref active) = runtime.active_claude {
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
                                let _ = self
                                    .send_text_message(
                                        &receive_id_type,
                                        &receive_id,
                                        &crate::t_fmt!("feishu.dir_changed", PATH = path_str),
                                    )
                                    .await;
                            }
                            Err(e) => {
                                let _ = self
                                    .send_text_message(
                                        &receive_id_type,
                                        &receive_id,
                                        &e.to_string(),
                                    )
                                    .await;
                            }
                        }
                    }
                }

                // --- /claude-history: resume existing session ---
                Some("resume") => {
                    let session_id = user_value
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if session_id.is_empty() {
                        // Start new session with the specified work_dir
                        let _work_dir = user_value
                            .get("work_dir")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&self.default_dir);
                        let cmd = format!("/claude");
                        let normalized = NormalizedMessage {
                            message_id: uuid::Uuid::new_v4().to_string(),
                            message_type: "text".to_string(),
                            content: cmd,
                            sender_open_id: String::new(),
                            sender_name: None,
                            chat_id: Some(chat_id.to_string()),
                            chat_type: Some("p2p".to_string()),
                            mentions: vec![],
                            raw: serde_json::json!({}),
                            receive_id_type: receive_id_type.to_string(),
                            receive_id: receive_id.clone(),
                        };
                        self.process_normalized_message(normalized).await;
                    } else {
                        // Resume existing session
                        let runtime = self
                            .get_channel(chat_id, &receive_id_type, &receive_id)
                            .await;
                        let default_work_dir =
                            crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                                .get_channel(&runtime.channel_session.id)
                                .map(|c| c.work_dir)
                                .unwrap_or_default();
                        let effective_dir = user_value
                            .get("work_dir")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&default_work_dir);

                        let resume_id = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                            .get_claude_session(&session_id)
                            .and_then(|s| s.claude_session_id.clone());

                        let mcp_ctx = crate::claude::mcp_server::McpContext {
                            feishu_app_id: self.config.app_id.clone(),
                            feishu_app_secret: self.config.app_secret.clone(),
                            chat_id: receive_id.clone(),
                            receive_id_type: receive_id_type.clone(),
                        };
                        match crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                            .start_claude_session_for_platform(
                                &runtime.channel_session.id,
                                &format!("Feishu {}", receive_id),
                                &self.default_dir,
                                self.claude_config.clone(),
                                self.show_thinking
                                    .load(std::sync::atomic::Ordering::Relaxed),
                                vec![],
                                resume_id,
                                Some(effective_dir.to_string()),
                                Some(mcp_ctx),
                            )
                            .await
                        {
                            Ok(active) => {
                                if let Some(mut entry) = self.channels.get_mut(chat_id) {
                                    entry.active_claude = Some(active.clone());
                                }
                                let _ = self
                                    .send_text_message(
                                        &receive_id_type,
                                        &receive_id,
                                        &crate::t_fmt!(
                                            "feishu.session_resumed",
                                            DIR = effective_dir
                                        ),
                                    )
                                    .await;
                            }
                            Err(e) => {
                                let _ = self
                                    .send_text_message(
                                        &receive_id_type,
                                        &receive_id,
                                        &crate::t_fmt!("builtin.failed_start_claude", ERR = e),
                                    )
                                    .await;
                            }
                        }
                    }
                }

                // --- /claude-history: delete session ---
                Some("delete_session") => {
                    let session_id = user_value
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !session_id.is_empty() {
                        let deleted = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                            .remove_claude_session(session_id);
                        let msg = if deleted {
                            crate::t!("feishu.session_deleted")
                        } else {
                            crate::t!("feishu.cannot_delete_active")
                        };
                        let _ = self
                            .send_text_message(&receive_id_type, &receive_id, msg)
                            .await;
                    }
                }

                // --- Permission card: allow / deny ---
                Some("allow") | Some("deny") => {
                    let request_id = user_value
                        .get("request_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let is_allow = cmd == Some("allow");
                    if !request_id.is_empty() {
                        let runtime = self
                            .get_channel(chat_id, &receive_id_type, &receive_id)
                            .await;
                        if let Some(ref active) = runtime.active_claude {
                            let ctrl = active.controller.lock().await;
                            if is_allow {
                                let allow_msg =
                                    crate::claude::protocol::build_permission_allow(request_id);
                                let _ = ctrl.send_input(allow_msg).await;
                            } else {
                                let deny_msg = crate::claude::protocol::build_permission_deny(
                                    request_id,
                                    "User denied permission",
                                );
                                let _ = ctrl.send_input(deny_msg).await;
                            }
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
                        if let Some(ref active) = runtime.active_claude {
                            let ctrl = active.controller.lock().await;
                            let msg = crate::claude::protocol::build_permission_allow(request_id);
                            let _ = ctrl.send_input(msg).await;
                        }
                    }
                }

                _ => {
                    debug!("Unhandled card action: {:?}", user_value);
                }
            }
        }
    }
}
