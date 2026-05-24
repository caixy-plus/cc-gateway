use anyhow::{Context, Result};
use bytes::BytesMut;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration as TokioDuration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tracing::{debug, info, warn};

use crate::claude::controller::ClaudeController;
use crate::claude::event_poller::{ClaudeEventPoller, EventPollSink};
use crate::command::router::{CommandAction, CommandRouter};
use crate::session::channel_manager::{ActiveClaudeRuntime, GLOBAL_CHANNEL_SESSIONS};
use crate::{t, t_fmt};

use super::*;

/// Feishu-specific sink for ClaudeEventPoller.
pub(crate) struct FeishuEventSink<'a> {
    platform: &'a FeishuPlatform,
    receive_id_type: String,
    receive_id: String,
    sender_open_id: String,
}

#[async_trait::async_trait]
impl<'a> EventPollSink for FeishuEventSink<'a> {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        if !text.trim().is_empty() {
            self.platform
                .send_text_message(&self.receive_id_type, &self.receive_id, text)
                .await?;
            crate::web::state::broadcast_event(
                &self.receive_id,
                "feishu",
                &self.receive_id,
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
        input: Option<&serde_json::Value>,
    ) -> Result<()> {
        let card = self.platform.build_permission_card(request_id, tool_name, input);
        self.platform
            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
            .await?;
        let ctx = PendingPermissionContext {
            request_id: request_id.to_string(),
            tool_name: tool_name.to_string(),
            chat_id: self.receive_id.clone(),
            sender_open_id: self.sender_open_id.clone(),
            created_at: Instant::now(),
        };
        self.platform.store_pending_permission(ctx);
        let notice = format!("Permission request: `{}`  ID: `{}`", tool_name, request_id);
        crate::web::state::broadcast_event(
            &self.receive_id,
            "feishu",
            &self.receive_id,
            "system",
            &notice,
        );
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        _options: &[String],
    ) -> Result<()> {
        let card = self.platform.build_confirm_card(request_id, prompt);
        self.platform
            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
            .await?;
        let interaction = interaction::PendingInteraction {
            request_id: request_id.to_string(),
            interaction_type: interaction::InteractionType::Confirm {
                prompt: prompt.to_string(),
            },
            state: interaction::InteractionState::Waiting,
            chat_id: self.receive_id.clone(),
            sender_open_id: self.sender_open_id.clone(),
            message_id: String::new(),
            created_at: Instant::now(),
        };
        self.platform.interaction_store.insert(interaction);
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        let card = self
            .platform
            .build_single_select_card(request_id, prompt, options);
        self.platform
            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
            .await?;
        let interaction = interaction::PendingInteraction {
            request_id: request_id.to_string(),
            interaction_type: interaction::InteractionType::SingleSelect {
                prompt: prompt.to_string(),
                options: options.to_vec(),
            },
            state: interaction::InteractionState::Waiting,
            chat_id: self.receive_id.clone(),
            sender_open_id: self.sender_open_id.clone(),
            message_id: String::new(),
            created_at: Instant::now(),
        };
        self.platform.interaction_store.insert(interaction);
        Ok(())
    }

    async fn on_question_request(
        &mut self,
        request_id: &str,
        questions: &[crate::claude::controller::QuestionItem],
    ) -> Result<()> {
        let first = &questions[0];
        let opts: Vec<String> = first.options.iter().map(|o| o.label.clone()).collect();
        if !opts.is_empty() && !first.multi_select {
            let card = self
                .platform
                .build_single_select_card(request_id, &first.question, &opts);
            self.platform
                .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
                .await?;
            let interaction = interaction::PendingInteraction {
                request_id: request_id.to_string(),
                interaction_type: interaction::InteractionType::SingleSelect {
                    prompt: first.question.clone(),
                    options: opts,
                },
                state: interaction::InteractionState::Waiting,
                chat_id: self.receive_id.clone(),
                sender_open_id: self.sender_open_id.clone(),
                message_id: String::new(),
                created_at: Instant::now(),
            };
            self.platform.interaction_store.insert(interaction);
        } else {
            let card = self
                .platform
                .build_text_input_hint_card(request_id, &first.question);
            self.platform
                .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
                .await?;
            let interaction = interaction::PendingInteraction {
                request_id: request_id.to_string(),
                interaction_type: interaction::InteractionType::TextInput {
                    prompt: first.question.clone(),
                },
                state: interaction::InteractionState::Waiting,
                chat_id: self.receive_id.clone(),
                sender_open_id: self.sender_open_id.clone(),
                message_id: String::new(),
                created_at: Instant::now(),
            };
            self.platform.interaction_store.insert(interaction);
        }
        Ok(())
    }

}

impl FeishuPlatform {
    pub(crate) async fn handle_event(
        &self,
        event: &Value,
    ) -> Result<()> {
        let normalized = match self.normalize_message(event) {
            Some(n) => n,
            None => {
                // Deduplicated or malformed event
                return Ok(());
            }
        };

        // Only allow private chats (p2p). Group chats are not supported.
        if normalized.chat_type.as_deref() != Some("p2p") {
            if !normalized.receive_id.is_empty() {
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, "请在私聊使用").await?;
            }
            return Ok(());
        }

        // Extract text content for routing
        let message_text = match normalized.message_type.as_str() {
            "text" => {
                // Parse JSON content for text messages (Feishu wraps text in JSON)
                serde_json::from_str::<Value>(&normalized.content)
                    .ok()
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
                    .unwrap_or_else(|| normalized.content.clone())
            }
            "post" => {
                // For post messages, try to extract plain text or use raw content
                normalized.content.clone()
            }
            _ => normalized.content.clone(),
        };

        let runtime = self.get_channel(&normalized.receive_id, &normalized.receive_id_type).await;
        let channel_id = runtime.channel_session.id.clone();

        // Build a router for this channel
        let router = if let Some(ref active) = runtime.active_claude {
            CommandRouter::new(active.controller.clone(), &self.default_dir)
        } else {
            // No active session: create a temporary router with a dummy controller
            // so that route() can still classify commands correctly.
            let dummy = Arc::new(Mutex::new(ClaudeController::new(
                self.claude_config.clone(),
                self.show_thinking.load(Ordering::Relaxed),
            )));
            CommandRouter::new(dummy, &self.default_dir)
        };

        let msg_id = normalized.message_id.clone();
        self.on_processing_start(&msg_id).await;
        let handle_result: Result<()> = async {
            let action = router.route(&message_text).await;

            match action {
            CommandAction::Reply(text) => {
                if !normalized.receive_id.is_empty() {
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &text).await?;
                }
            }
            CommandAction::UnknownCommand(_) => {
                let text = t!("feishu.unknown_command");
                if !normalized.receive_id.is_empty() {
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &text).await?;
                }
            }
            CommandAction::NoOp => {}
            CommandAction::StopSession => {
                // Explicitly stop the Claude process before clearing state.
                // stop_active_claude_session only handles WebUI controllers;
                // for Feishu we must call controller.stop_session() ourselves
                // to kill the Claude subprocess.
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    let _ = ctrl.stop_session().await;
                }
                let _ = GLOBAL_CHANNEL_SESSIONS.stop_active_claude_session(&channel_id).await;
                if let Some(mut rt) = self.channels.get_mut(&normalized.receive_id) {
                    rt.active_claude = None;
                }
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, t!("builtin.session_stopped")).await?;
            }
            CommandAction::ShowThinking => {
                self.show_thinking.store(true, Ordering::Relaxed);
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(true);
                }
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, t!("builtin.thinking_enabled")).await?;
            }
            CommandAction::HideThinking => {
                self.show_thinking.store(false, Ordering::Relaxed);
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.set_show_thinking(false);
                }
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, t!("builtin.thinking_disabled")).await?;
            }
            CommandAction::ChangeDir(path) => {
                let base = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                let target = std::path::PathBuf::from(&base).join(&path);
                let canonical = target.canonicalize().unwrap_or(target);
                if !canonical.is_dir() {
                    let reply = t_fmt!("builtin.invalid_path", PATH = canonical.display());
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &reply).await?;
                    return Ok(());
                }
                let path_str = canonical.to_string_lossy().to_string();
                if let Err(e) = crate::claude::controller::ensure_under_home(&path_str) {
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &e.to_string()).await?;
                    return Ok(());
                }
                GLOBAL_CHANNEL_SESSIONS.switch_work_dir(&channel_id, canonical).await?;
                if let Some(mut rt) = self.channels.get_mut(&normalized.receive_id) {
                    rt.channel_session.work_dir = path_str.clone();
                }
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &t_fmt!("builtin.dir_changed", PATH = path_str)).await?;
            }
            CommandAction::ChangeDirDefault => {
                let dir = shellexpand::tilde(&self.default_dir).to_string();
                GLOBAL_CHANNEL_SESSIONS.switch_work_dir(&channel_id, std::path::PathBuf::from(&dir)).await?;
                if let Some(mut rt) = self.channels.get_mut(&normalized.receive_id) {
                    rt.channel_session.work_dir = dir.clone();
                }
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &t_fmt!("builtin.dir_changed", PATH = dir)).await?;
            }
            CommandAction::PrintWorkingDir => {
                let dir = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &t_fmt!("builtin.current_dir", DIR = dir)).await?;
            }
            CommandAction::ListDir { path } => {
                let dir = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                let target = path.unwrap_or_else(|| std::path::PathBuf::from(&dir));
                match crate::command::builtin::list_directory_paths(&target.to_string_lossy()) {
                    Ok(dirs) => {
                        if dirs.is_empty() {
                            self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, t!("builtin.no_subdirs")).await?;
                        } else {
                            let card = self.build_dir_select_card(&dirs, 0, &dir, &normalized.receive_id_type, &normalized.receive_id);
                            self.send_interactive_card(&normalized.receive_id_type, &normalized.receive_id, &card).await?;
                        }
                    }
                    Err(e) => {
                        self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &t_fmt!("builtin.failed_list_dir", ERR = e)).await?;
                    }
                }
            }
            CommandAction::MakeDir(path) => {
                let base = if runtime.channel_session.work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    runtime.channel_session.work_dir.clone()
                };
                let target = std::path::PathBuf::from(&base).join(&path);
                let target_str = target.to_string_lossy().to_string();
                if let Err(e) = crate::claude::controller::ensure_under_home(&target_str) {
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &e.to_string()).await?;
                    return Ok(());
                }
                match std::fs::create_dir_all(&target) {
                    Ok(()) => {
                        self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &t_fmt!("builtin.dir_created", PATH = target_str)).await?;
                    }
                    Err(e) => {
                        self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, &t_fmt!("builtin.failed_create_dir", ERR = e)).await?;
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

                let mcp_ctx = crate::claude::mcp_server::McpContext {
                    feishu_app_id: self.config.app_id.clone(),
                    feishu_app_secret: self.config.app_secret.clone(),
                    chat_id: normalized.receive_id.clone(),
                    receive_id_type: normalized.receive_id_type.clone(),
                };
                match GLOBAL_CHANNEL_SESSIONS
                    .create_and_start_claude_session(
                        &channel_id,
                        "Feishu",
                        self.claude_config.clone(),
                        self.show_thinking.load(Ordering::Relaxed),
                        args,
                        None,
                        Some(mcp_ctx),
                    )
                    .await
                {
                    Ok((claude_session, controller)) => {
                        // Sync work_dir if different
                        if effective_dir != claude_session.work_dir {
                            let ctrl = controller.lock().await;
                            ctrl.init_work_dir(effective_dir.clone()).await;
                        }
                        let router = Arc::new(CommandRouter::new(controller.clone(), &self.default_dir));
                        let active = ActiveClaudeRuntime {
                            claude_session: claude_session.clone(),
                            controller: controller.clone(),
                            router,
                        };
                        if let Some(mut rt) = self.channels.get_mut(&normalized.receive_id) {
                            rt.active_claude = Some(active.clone());
                        }
                        self.send_text_message(
                            &normalized.receive_id_type,
                            &normalized.receive_id,
                            &t_fmt!("builtin.session_started", DIR = claude_session.work_dir),
                        )
                        .await?;
                    }
                    Err(e) => {
                        self.send_text_message(
                            &normalized.receive_id_type,
                            &normalized.receive_id,
                            &t_fmt!("builtin.failed_start_claude", ERR = e),
                        )
                        .await?;
                    }
                }
            }
            CommandAction::ShowClaudeHistory => {
                let sessions = GLOBAL_CHANNEL_SESSIONS.list_claude_sessions_by_channel(&channel_id);
                if sessions.is_empty() {
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, t!("feishu.no_sessions")).await?;
                } else {
                    let card = self.build_session_history_card(&sessions, &normalized.receive_id_type, &normalized.receive_id);
                    self.send_interactive_card(&normalized.receive_id_type, &normalized.receive_id, &card).await?;
                }
            }
            CommandAction::ForwardToClaude(text) => {
                if let Some(ref active) = runtime.active_claude {
                    let ctrl = active.controller.lock().await;
                    ctrl.send_message(&text).await?;
                    drop(ctrl);
                    GLOBAL_CHANNEL_SESSIONS.touch_claude_session(&active.claude_session.id);

                    let _guard = runtime.poll_lock.lock().await;
                    let mut sink = FeishuEventSink {
                        platform: self,
                        receive_id_type: normalized.receive_id_type.clone(),
                        receive_id: normalized.receive_id.clone(),
                        sender_open_id: normalized.sender_open_id.clone(),
                    };
                    let poller = {
                        let ctrl = active.controller.lock().await;
                        ClaudeEventPoller::from_controller(&*ctrl)
                    };
                    poller.run(&mut sink).await?;
                } else {
                    self.send_text_message(&normalized.receive_id_type, &normalized.receive_id, t!("forward.no_session")).await?;
                }
            }
        }
            Ok(())
        }.await;
        self.on_processing_complete(&msg_id, handle_result.is_ok()).await;
        handle_result
    }

    // -----------------------------------------------------------------------
    // WebSocket loop
    // -----------------------------------------------------------------------

    pub(crate) async fn run_websocket(&self, ws_url: &str, client_config: WsClientConfig) -> Result<()> {
        let mut current_url = ws_url.to_string();
        let mut current_config = client_config;
        let mut retry_count: u32 = 0;

        loop {
            let service_id = Self::extract_service_id(&current_url).unwrap_or(0);
            match self.ws_connection_loop(&current_url, &current_config, service_id).await {
                Ok(()) => {
                    info!("Feishu WebSocket closed gracefully");
                    break;
                }
                Err(e) => {
                    retry_count += 1;
                    let err_msg = e.to_string();
                    let is_network_error = err_msg.contains("timeout")
                        || err_msg.contains("connect")
                        || err_msg.contains("read error");

                    // Network errors: short wait, no endpoint refresh (URL didn't change)
                    // Auth/protocol errors: refresh endpoint before retry
                    if is_network_error {
                        warn!(
                            "WebSocket network error: {}. Reconnecting in 3s... (retry #{})",
                            e, retry_count
                        );
                        sleep(TokioDuration::from_secs(3)).await;
                    } else {
                        warn!(
                            "WebSocket error: {}. Refreshing endpoint and reconnecting in 3s... (retry #{})",
                            e, retry_count
                        );
                        sleep(TokioDuration::from_secs(3)).await;

                        match self.get_ws_endpoint().await {
                            Ok((u, cfg)) => {
                                current_url = u;
                                current_config = cfg;
                            }
                            Err(e2) => {
                                warn!("Failed to refresh WS endpoint: {}", e2);
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn extract_service_id(url: &str) -> Option<i32> {
        url.split('?').nth(1)?.split('&').find(|p| p.starts_with("service_id="))?
            .strip_prefix("service_id=")?.parse().ok()
    }

    pub(crate) async fn ws_connection_loop(&self, ws_url: &str, client_config: &WsClientConfig, service_id: i32) -> Result<()> {
        // Build WebSocket request (no User-Agent, matching Go SDK behavior)
        let req = ws_url.into_client_request()
            .context("Invalid WebSocket URL")?;
        let (ws_stream, response) = timeout(
                TokioDuration::from_secs(5),
                connect_async(req)
            )
            .await
            .map_err(|_| anyhow::anyhow!("WebSocket connect timeout (5s)"))?
            .context("WebSocket connect failed")?;

        info!("Feishu WebSocket connected, response status={:?}, headers={:?}", response.status(), response.headers());

        let (write, mut read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));
        let ping_interval = Arc::new(std::sync::atomic::AtomicU64::new(client_config.ping_interval.max(1) as u64));

        // Send initial PING immediately so Feishu server acknowledges the new connection
        {
            let ping = build_ping_frame(service_id);
            let mut buf = BytesMut::new();
            ping.encode(&mut buf);
            let mut w = write.lock().await;
            if w.send(WsMessage::Binary(buf.freeze())).await.is_ok() {
                info!("Sent initial PING seq_id={} service_id={} immediately after connect", ping.seq_id, service_id);
            }
            drop(w);
        }

        // Small delay (200ms) to stabilize the connection before entering the read loop
        sleep(TokioDuration::from_millis(200)).await;

        // Spawn heartbeat writer
        let write_for_heartbeat = write.clone();
        let ping_interval_for_heartbeat = ping_interval.clone();
        let mut heartbeat_handle = {
            tokio::spawn(async move {
                loop {
                    let interval_secs = ping_interval_for_heartbeat.load(std::sync::atomic::Ordering::Relaxed);
                    sleep(TokioDuration::from_secs(interval_secs)).await;
                    let ping = build_ping_frame(service_id);
                    let mut buf = BytesMut::new();
                    ping.encode(&mut buf);
                    let mut w = write_for_heartbeat.lock().await;
                    if w.send(WsMessage::Binary(buf.freeze())).await.is_err() {
                        break;
                    }
                    debug!("Sent PING seq_id={} service_id={}", ping.seq_id, service_id);
                    drop(w);
                }
            })
        };

        // Race: read loop vs heartbeat failure.
        // When the network drops, the heartbeat writer's next send will fail,
        // which exits the heartbeat task. select! fires immediately instead of
        // waiting for the read timeout (~20s). Worst case detection: ping_interval.
        let read_loop = async {
            loop {
                let read_timeout_duration = TokioDuration::from_secs(
                    (ping_interval.load(std::sync::atomic::Ordering::Relaxed) * 2).max(5)
                );
                match timeout(read_timeout_duration, read.next()).await {
                    Ok(Some(Ok(msg))) => {
                        match msg {
                            WsMessage::Binary(data) => {
                                debug!("WS raw binary len={}", data.len());
                                let hex_dump: String = data.iter().take(100).map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(" ");
                                debug!("WS raw binary hex (first 100 bytes): {}", hex_dump);
                                if let Some(frame) = Frame::decode(&data) {
                                    debug!("Decoded frame: seq_id={} log_id={} service={} method={} headers={:?} payload_encoding={:?} payload_type={:?} payload_len={:?} log_id_new={:?}",
                                        frame.seq_id, frame.log_id, frame.service, frame.method,
                                        frame.headers.iter().map(|h| format!("{}={}", h.key, h.value)).collect::<Vec<_>>(),
                                        frame.payload_encoding, frame.payload_type,
                                        frame.payload.as_ref().map(|v| v.len()),
                                        frame.log_id_new
                                    );
                                    if frame.method == METHOD_CONTROL {
                                        if let Some(ref payload) = frame.payload {
                                            if let Ok(cfg) = serde_json::from_slice::<WsClientConfig>(payload) {
                                                debug!("Received pong with ClientConfig: ping_interval={}s", cfg.ping_interval);
                                                ping_interval.store(cfg.ping_interval.max(1) as u64, std::sync::atomic::Ordering::Relaxed);
                                            }
                                        }
                                    }
                                    if let Some(ack) = self.handle_frame(&frame).await? {
                                        let mut buf = BytesMut::new();
                                        ack.encode(&mut buf);
                                        let mut w = write.lock().await;
                                        w.send(WsMessage::Binary(buf.freeze())).await.ok();
                                        drop(w);
                                        debug!("Sent ACK for seq_id={}", frame.seq_id);
                                    }
                                } else {
                                    warn!("Received invalid protobuf frame");
                                }
                            }
                            WsMessage::Close(_) => {
                                info!("WebSocket close frame received");
                                break;
                            }
                            WsMessage::Ping(data) => {
                                let mut w = write.lock().await;
                                w.send(WsMessage::Pong(data)).await.ok();
                            }
                            WsMessage::Text(text) => {
                                debug!("Unexpected text frame: {}", text);
                            }
                            _ => {}
                        }
                    }
                    Ok(Some(Err(e))) => {
                        return Err(anyhow::anyhow!("WebSocket read error: {}", e));
                    }
                    Ok(None) => {
                        info!("WebSocket stream ended");
                        break;
                    }
                    Err(_) => {
                        warn!("WebSocket read timeout: no data received for {}s", read_timeout_duration.as_secs());
                        return Err(anyhow::anyhow!("WebSocket read timeout"));
                    }
                }
            }
            Ok(())
        };

        let result: Result<()> = tokio::select! {
            r = read_loop => r,
            _ = &mut heartbeat_handle => {
                warn!("WebSocket heartbeat writer terminated — connection lost");
                Err(anyhow::anyhow!("WebSocket heartbeat writer terminated (connection lost)"))
            }
        };

        heartbeat_handle.abort();
        result
    }

    // -----------------------------------------------------------------------
    // Frame handling
    // -----------------------------------------------------------------------

    pub(crate) async fn handle_frame(&self, frame: &Frame) -> Result<Option<Frame>> {
        debug!(
            "Frame seq_id={} method={} service={} payload_len={:?}",
            frame.seq_id,
            frame.method,
            frame.service,
            frame.payload.as_ref().map(|v| v.len())
        );

        match frame.method {
            METHOD_CONTROL => {
                // Control frames (pong with ClientConfig) are handled in ws_connection_loop
                debug!("Received control frame seq_id={}, type={:?}", frame.seq_id, frame.headers.iter().find(|h| h.key == "type").map(|h| &h.value));
            }
            METHOD_DATA => {
                // Send ACK immediately; business processing is done in the background
                // so Feishu does not time out and resend while Claude is thinking.
                if let Some(ref payload) = frame.payload {
                    let payload = payload.clone();
                    let platform = self.clone();
                    tokio::spawn(async move {
                        let event_type = serde_json::from_slice::<Value>(&payload)
                            .ok()
                            .and_then(|v| v.get("header").cloned())
                            .and_then(|h| h.get("event_type").cloned())
                            .and_then(|v| v.as_str().map(|s| s.to_string()));

                        match event_type.as_deref() {
                            Some("im.message.receive_v1") => {
                                if let Err(e) = platform.handle_im_payload(&payload).await {
                                    warn!("[Feishu] IM payload failed: {}", e);
                                }
                            }
                            Some("card.action.trigger") => {
                                if let Err(e) = platform.handle_card_payload(&payload).await {
                                    warn!("[Feishu] Card payload failed: {}", e);
                                }
                            }
                            _ => {
                                // Unknown event type – try both handlers as fallback
                                if let Err(e) = platform.handle_im_payload(&payload).await {
                                    warn!("[Feishu] IM payload failed: {}", e);
                                }
                                if let Err(e) = platform.handle_card_payload(&payload).await {
                                    warn!("[Feishu] Card payload failed: {}", e);
                                }
                            }
                        }
                    });
                }
                return Ok(Some(build_ack_frame(&frame)));
            }
            _ => {
                debug!(
                    "Unhandled frame method={} service={}",
                    frame.method, frame.service
                );
            }
        }
        Ok(None)
    }

    pub(crate) async fn handle_im_payload(&self, payload: &[u8]) -> Result<()> {
        let event_json: Value =
            serde_json::from_slice(payload).context("Failed to parse IM event wrapper")?;
        self.handle_event(&event_json).await
    }

    pub(crate) async fn handle_card_payload(&self, payload: &[u8]) -> Result<()> {
        let wrapper: EventWrapper =
            serde_json::from_slice(payload).context("Failed to parse card event wrapper")?;
        let event_json = match wrapper.event {
            Some(v) => v,
            None => return Ok(()),
        };

        let open_id = event_json
            .get("operator")
            .and_then(|o| o.get("open_id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if !self.is_allowed_sender(&open_id) {
            debug!("Card action from {} not allowed", open_id);
            return Ok(());
        }

        let action_obj = event_json.get("action").cloned();
        let context = event_json.get("context");

        // Handle directory selection / session resume cards
        if let Some(ref action_value) = action_obj {
            let user_value = action_value.get("value").unwrap_or(action_value);
            if let Some(cmd) = user_value.get("cmd").and_then(|v| v.as_str()) {
                let receive_id = user_value
                    .get("chat_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| context.and_then(|c| c.get("open_chat_id")).and_then(|v| v.as_str()))
                    .unwrap_or("");
                let receive_id_type = user_value
                    .get("receive_id_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("chat_id");

                if !receive_id.is_empty() {
                    match cmd {
                        "resume" => {
                            let session_id = user_value.get("session_id").and_then(|v| v.as_str()).unwrap_or("");

                            if session_id.is_empty() {
                                // New session: work_dir selected from card, no existing session to resume
                                let wd = user_value.get("work_dir").and_then(|v| v.as_str()).map(|s| s.to_string());
                                if let Some(ref wd) = wd {
                                    GLOBAL_CHANNEL_SESSIONS.update_channel_work_dir(
                                        &self.get_channel(receive_id, receive_id_type).await.channel_session.id,
                                        wd,
                                    );
                                    if let Some(mut rt) = self.channels.get_mut(receive_id) {
                                        rt.channel_session.work_dir = wd.clone();
                                    }
                                }

                                let channel_id = self.get_channel(receive_id, receive_id_type).await.channel_session.id.clone();
                                let mcp_ctx = crate::claude::mcp_server::McpContext {
                                    feishu_app_id: self.config.app_id.clone(),
                                    feishu_app_secret: self.config.app_secret.clone(),
                                    chat_id: receive_id.to_string(),
                                    receive_id_type: receive_id_type.to_string(),
                                };
                                match GLOBAL_CHANNEL_SESSIONS.create_and_start_claude_session(
                                    &channel_id,
                                    "Feishu",
                                    self.claude_config.clone(),
                                    self.show_thinking.load(Ordering::Relaxed),
                                    vec![],
                                    None,
                                    Some(mcp_ctx),
                                ).await {
                                    Ok((claude_session, controller)) => {
                                        let router = Arc::new(CommandRouter::new(controller.clone(), &self.default_dir));
                                        let active = ActiveClaudeRuntime {
                                            claude_session: claude_session.clone(),
                                            controller: controller.clone(),
                                            router,
                                        };
                                        if let Some(mut rt) = self.channels.get_mut(receive_id) {
                                            rt.active_claude = Some(active.clone());
                                        }
                                        self.send_text_message(receive_id_type, receive_id, &t_fmt!("builtin.session_started", DIR = claude_session.work_dir)).await?;
                                    }
                                    Err(e) => {
                                        self.send_text_message(receive_id_type, receive_id, &t_fmt!("builtin.failed_start_claude", ERR = e)).await?;
                                    }
                                }
                            } else {
                                // Resume existing session — reuses the same DB record, no new session created
                                let mcp_ctx = crate::claude::mcp_server::McpContext {
                                    feishu_app_id: self.config.app_id.clone(),
                                    feishu_app_secret: self.config.app_secret.clone(),
                                    chat_id: receive_id.to_string(),
                                    receive_id_type: receive_id_type.to_string(),
                                };
                                match GLOBAL_CHANNEL_SESSIONS.resume_claude_session(
                                    session_id,
                                    self.claude_config.clone(),
                                    self.show_thinking.load(Ordering::Relaxed),
                                    Some(mcp_ctx),
                                ).await {
                                    Ok((claude_session, controller)) => {
                                        let router = Arc::new(CommandRouter::new(controller.clone(), &self.default_dir));
                                        let active = ActiveClaudeRuntime {
                                            claude_session: claude_session.clone(),
                                            controller: controller.clone(),
                                            router,
                                        };
                                        if let Some(mut rt) = self.channels.get_mut(receive_id) {
                                            rt.active_claude = Some(active.clone());
                                        }
                                        self.send_text_message(receive_id_type, receive_id, &t_fmt!("builtin.session_resumed", DIR = claude_session.work_dir)).await?;
                                    }
                                    Err(e) => {
                                        self.send_text_message(receive_id_type, receive_id, &t_fmt!("builtin.failed_start_claude", ERR = e)).await?;
                                    }
                                }
                            }
                            return Ok(());
                        }
                        "cd" => {
                            if let Some(path) = user_value.get("path").and_then(|v| v.as_str()) {
                                let runtime = self.get_channel(receive_id, receive_id_type).await;
                                GLOBAL_CHANNEL_SESSIONS.update_channel_work_dir(&runtime.channel_session.id, path);
                                if let Some(mut rt) = self.channels.get_mut(receive_id) {
                                    rt.channel_session.work_dir = path.to_string();
                                }
                                self.send_text_message(receive_id_type, receive_id, &t_fmt!("feishu.dir_changed", PATH = path)).await?;
                                return Ok(());
                            }
                        }
                        "ll_page" => {
                            let page = user_value.get("page").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                            let dir = user_value.get("dir").and_then(|v| v.as_str()).unwrap_or("");
                            if !dir.is_empty() {
                                let dirs = crate::command::builtin::list_directory_paths(dir).unwrap_or_default();
                                if !dirs.is_empty() {
                                    let card = self.build_dir_select_card(&dirs, page, dir, receive_id_type, receive_id);
                                    self.send_interactive_card(receive_id_type, receive_id, &card).await?;
                                }
                                return Ok(());
                            }
                        }
                        "delete_session" => {
                            let session_id = user_value.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                            if !session_id.is_empty() {
                                // Delete history file
                                let file_id = GLOBAL_CHANNEL_SESSIONS
                                    .get_claude_session(session_id)
                                    .and_then(|s| s.claude_session_id)
                                    .unwrap_or_else(|| session_id.to_string());
                                if let Some(home) = dirs::home_dir() {
                                    let history_file = home.join(".cc-gateway").join("history").join(format!("{}.jsonl", file_id));
                                    let _ = std::fs::remove_file(&history_file);
                                }
                                GLOBAL_CHANNEL_SESSIONS.remove_claude_session(session_id);
                                self.send_text_message(receive_id_type, receive_id, t!("feishu.session_deleted")).await?;
                            }
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        // Handle interactive card callbacks
        if let Some(ref action_value) = action_obj {
            let user_value = action_value.get("value").unwrap_or(action_value);
            if let Some(action) = user_value.get("action").and_then(|v| v.as_str()) {
                if let Some(request_id) = user_value.get("request_id").and_then(|v| v.as_str()) {
                    let receive_id = user_value
                        .get("chat_id")
                        .and_then(|v| v.as_str())
                        .or_else(|| context.and_then(|c| c.get("open_chat_id")).and_then(|v| v.as_str()))
                        .unwrap_or("");
                    let receive_id_type = user_value
                        .get("receive_id_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("chat_id");

                    if !receive_id.is_empty() {
                        match action {
                            "approve_once" | "approve_session" | "approve_always" | "deny" => {
                                if let Some(ctx) = self.take_pending_permission(request_id) {
                                    let runtime = self.get_channel(&ctx.chat_id, "chat_id").await;
                                    if let Some(ref active) = runtime.active_claude {
                                        let ctrl = active.controller.lock().await;
                                        let msg = if action == "deny" {
                                            interaction::build_deny_response(request_id, "User denied")
                                        } else {
                                            interaction::build_allow_response(request_id)
                                        };
                                        let _ = ctrl.send_input(msg).await;
                                        drop(ctrl);
                                    }
                                    let reply = if action == "deny" { "已拒绝".to_string() } else { format!("已允许执行: {}", ctx.tool_name) };
                                    let _ = self.send_text_message(receive_id_type, receive_id, &reply).await;
                                    return Ok(());
                                }
                            }
                            "confirm" => {
                                if let Some(answer) = user_value.get("answer").and_then(|v| v.as_bool()) {
                                    if let Some(_pending) = self.interaction_store.take(request_id) {
                                        let runtime = self.get_channel(receive_id, receive_id_type).await;
                                        if let Some(ref active) = runtime.active_claude {
                                            let ctrl = active.controller.lock().await;
                                            let answer_val = serde_json::json!(answer);
                                            let msg = interaction::build_select_response(request_id, answer_val);
                                            let _ = ctrl.send_input(msg).await;
                                            drop(ctrl);
                                        }
                                        let reply = if answer { "已确认" } else { "已取消" };
                                        let _ = self.send_text_message(receive_id_type, receive_id, reply).await;
                                        return Ok(());
                                    }
                                }
                            }
                            "select" => {
                                if let Some(answer) = user_value.get("answer").and_then(|v| v.as_str()) {
                                    if let Some(_pending) = self.interaction_store.take(request_id) {
                                        let runtime = self.get_channel(receive_id, receive_id_type).await;
                                        if let Some(ref active) = runtime.active_claude {
                                            let ctrl = active.controller.lock().await;
                                            let answer_val = serde_json::json!(answer);
                                            let msg = interaction::build_select_response(request_id, answer_val);
                                            let _ = ctrl.send_input(msg).await;
                                            drop(ctrl);
                                        }
                                        let _ = self.send_text_message(receive_id_type, receive_id, &format!("已选择: {}", answer)).await;
                                        return Ok(());
                                    }
                                }
                            }
                            "cancel_text_input" => {
                                if let Some(_pending) = self.interaction_store.take(request_id) {
                                    let _ = self.send_text_message(receive_id_type, receive_id, "已取消").await;
                                    return Ok(());
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
