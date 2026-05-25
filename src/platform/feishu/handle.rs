use anyhow::{Context, Result};
use dashmap::DashMap;
use serde_json::{json, Value};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::{sleep, timeout, Duration as TokioDuration};
use tracing::{debug, error, info, warn};

use crate::platform::proto::{Frame, Header};

use super::auth_middleware;
use super::interaction;
use super::{
    AnomalyTracker, BotInfo, BotInfoResp, ChatItem, DedupCache, FeishuChannelRuntime,
    FeishuPlatform, RateLimiter, ReactionCreateResp, WsClientConfig, WsEndpointResp,
    FEISHU_CHUNK_DELAY_MS, FEISHU_MAX_TEXT_CHARS, METHOD_CONTROL, REACTION_FAILURE,
    REACTION_TYPING,
};
use crate::config::model::{ClaudeConfig, FeishuConfig};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

impl FeishuPlatform {
    pub fn new(
        config: FeishuConfig,
        default_dir: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
    ) -> Self {
        let token_manager = auth_middleware::TokenManager::new(config.clone());
        let http_client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with(auth_middleware::FeishuAuthMiddleware::new(
                token_manager.clone(),
            ))
            .build();
        Self {
            config,
            default_dir: default_dir.to_string(),
            claude_config,
            show_thinking: Arc::new(AtomicBool::new(show_thinking)),
            http_client,
            dedup_cache: Arc::new(DedupCache::new(300)),
            pending_permissions: Arc::new(DashMap::new()),
            interaction_store: Arc::new(interaction::InteractionStore::new()),
            token_manager,
            pending_reactions: Arc::new(DashMap::new()),
            bot_identity: Arc::new(RwLock::new(None)),
            rate_limiter: Arc::new(RateLimiter::new(60, 60)),
            anomaly_tracker: Arc::new(AnomalyTracker::new(25, 21600)),
            channels: Arc::new(DashMap::new()),
        }
    }

    pub(crate) async fn get_channel(
        &self,
        chat_id: &str,
        receive_id_type: &str,
        receive_id: &str,
    ) -> FeishuChannelRuntime {
        if let Some(mut runtime) = self.channels.get_mut(chat_id) {
            runtime.receive_id_type = receive_id_type.to_string();
            runtime.receive_id = receive_id.to_string();
            return runtime.clone();
        }
        let channel = GLOBAL_CHANNEL_SESSIONS
            .get_or_create_platform_channel("feishu", chat_id, &self.default_dir)
            .await;
        let runtime =
            FeishuChannelRuntime::new(channel, receive_id_type.to_string(), receive_id.to_string());
        self.channels.insert(chat_id.to_string(), runtime.clone());
        runtime
    }

    pub(crate) fn spawn_deliver_listener(&self) {
        let platform = self.clone();
        crate::platform::spawn_deliver_listener("feishu", move |channel_id, text| {
            let platform = platform.clone();
            tokio::spawn(async move {
                let receive_id_type = if let Some(runtime) = platform.channels.get(&channel_id) {
                    runtime.receive_id_type.clone()
                } else {
                    "chat_id".to_string()
                };
                let _ = platform
                    .send_text_message(&receive_id_type, &channel_id, &text)
                    .await;
            });
        });
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Feishu platform...");
        self.spawn_deliver_listener();

        loop {
            let (ws_url, client_config) = match self.get_ws_endpoint().await {
                Ok(v) => v,
                Err(e) => {
                    error!(
                        "Failed to get Feishu WebSocket endpoint: {}, retrying in 5s...",
                        e
                    );
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            info!("Feishu WebSocket endpoint: {}", ws_url);

            match self.run_websocket(&ws_url, client_config).await {
                Ok(()) => {
                    warn!("Feishu WebSocket disconnected, reconnecting in 5s...");
                }
                Err(e) => {
                    error!("Feishu WebSocket error: {}, reconnecting in 5s...", e);
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    // -----------------------------------------------------------------------
    // HTTP API methods
    // -----------------------------------------------------------------------

    pub(crate) async fn get_tenant_access_token(&self) -> Result<String> {
        self.token_manager.get_tenant_access_token().await
    }

    pub(crate) async fn get_ws_endpoint(&self) -> Result<(String, WsClientConfig)> {
        let url = "https://open.feishu.cn/callback/ws/endpoint";
        let resp = timeout(
            TokioDuration::from_secs(10),
            self.http_client
                .post(url)
                .header("locale", "zh")
                .json(&serde_json::json!({
                    "AppID": &self.config.app_id,
                    "AppSecret": &self.config.app_secret,
                }))
                .send(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Request WebSocket endpoint timeout (10s)"))?
        .context("Failed to request WebSocket endpoint")?;

        let body_text = resp
            .text()
            .await
            .context("Failed to read WebSocket endpoint response body")?;
        debug!("Feishu WS endpoint raw response: {}", body_text);
        let data: WsEndpointResp = serde_json::from_str(&body_text).with_context(|| {
            format!("Failed to parse WebSocket endpoint response: {}", body_text)
        })?;

        if data.code != 0 {
            anyhow::bail!("Feishu WS endpoint error: {} - {}", data.code, data.msg);
        }

        let endpoint = data
            .data
            .ok_or_else(|| anyhow::anyhow!("Feishu WS endpoint response missing data"))?;
        let ws_url = endpoint
            .url
            .ok_or_else(|| anyhow::anyhow!("Feishu WS endpoint response missing URL"))?;
        let client_config = endpoint.client_config.unwrap_or(WsClientConfig {
            reconnect_count: 10,
            reconnect_interval: 5,
            reconnect_nonce: 5,
            ping_interval: 30,
        });
        Ok((ws_url, client_config))
    }

    pub async fn send_text_message(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        text: &str,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let chunks = split_text_into_chunks(text, FEISHU_MAX_TEXT_CHARS);
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                sleep(TokioDuration::from_millis(FEISHU_CHUNK_DELAY_MS)).await;
            }
            self.send_text_message_raw(receive_id_type, receive_id, chunk)
                .await?;
        }
        Ok(())
    }

    async fn send_text_message_raw(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        text: &str,
    ) -> Result<()> {
        match self
            .send_text_message_raw_inner(receive_id_type, receive_id, text)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if auth_middleware::TokenManager::is_auth_error(&e) => {
                self.token_manager.invalidate_token_cache().await;
                self.send_text_message_raw_inner(receive_id_type, receive_id, text)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn send_text_message_raw_inner(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        text: &str,
    ) -> Result<()> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", receive_id_type)])
            .json(&json!({
                "receive_id": receive_id,
                "content": json!({"text": text}).to_string(),
                "msg_type": "text",
            }))
            .send()
            .await
            .context("Failed to send Feishu text message")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            anyhow::bail!(
                "Feishu send text message failed: {} - {}",
                status,
                body_text
            );
        }
        let body: Value =
            serde_json::from_str(&body_text).unwrap_or_else(|_| serde_json::json!({"code": -1}));
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu API error (send text): code={}, msg={}", code, msg);
        }

        Ok(())
    }

    pub async fn send_post_message(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        content: &Value,
    ) -> Result<()> {
        match self
            .send_post_message_inner(receive_id_type, receive_id, content)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if auth_middleware::TokenManager::is_auth_error(&e) => {
                self.token_manager.invalidate_token_cache().await;
                self.send_post_message_inner(receive_id_type, receive_id, content)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn send_post_message_inner(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        content: &Value,
    ) -> Result<()> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", receive_id_type)])
            .json(&json!({
                "receive_id": receive_id,
                "content": content.to_string(),
                "msg_type": "post",
            }))
            .send()
            .await
            .context("Failed to send Feishu post message")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Feishu send post message failed: {} - {}", status, body);
        }

        Ok(())
    }

    pub async fn send_interactive_card(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        card_json: &Value,
    ) -> Result<()> {
        if receive_id.is_empty() {
            anyhow::bail!("Cannot send card: receive_id is empty");
        }
        match self
            .send_interactive_card_inner(receive_id_type, receive_id, card_json)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) if auth_middleware::TokenManager::is_auth_error(&e) => {
                self.token_manager.invalidate_token_cache().await;
                self.send_interactive_card_inner(receive_id_type, receive_id, card_json)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn send_interactive_card_inner(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        card_json: &Value,
    ) -> Result<()> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let request_body = json!({
            "receive_id": receive_id,
            "content": card_json.to_string(),
            "msg_type": "interactive",
        });
        debug!(
            "Sending interactive card to receive_id_type={} receive_id={}, body={}",
            receive_id_type, receive_id, request_body
        );
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", receive_id_type)])
            .json(&request_body)
            .send()
            .await
            .context("Failed to send Feishu interactive card")?;

        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        let body: Value =
            serde_json::from_str(&body_text).unwrap_or_else(|_| serde_json::json!({"code": -1}));
        if !status.is_success() {
            anyhow::bail!(
                "Feishu send interactive card failed: {} - {}",
                status,
                body_text
            );
        }
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu API error (send card): code={}, msg={}", code, msg);
        }

        Ok(())
    }

    pub async fn shutdown_all_sessions(&self) {
        for entry in self.channels.iter() {
            let chat_id = entry.key().clone();
            let runtime = entry.value().clone();
            drop(entry);

            if let Some(target) = runtime.shutdown_notice_target() {
                let _ = self
                    .send_text_message(
                        &target.receive_id_type,
                        &target.receive_id,
                        crate::t!("builtin.shutdown_notice"),
                    )
                    .await;

                let Some(ref active) = runtime.active_claude else {
                    continue;
                };
                let ctrl = active.controller.lock().await;
                match tokio::time::timeout(TokioDuration::from_millis(500), ctrl.stop_session())
                    .await
                {
                    Ok(Ok(())) => info!("[Feishu] Session {} stopped gracefully", chat_id),
                    Ok(Err(e)) => warn!("[Feishu] Session {} stop error: {}", chat_id, e),
                    Err(_) => warn!("[Feishu] Session {} stop timed out, killing", chat_id),
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // File upload & send
    // -----------------------------------------------------------------------

    /// Upload a file to Feishu and return the file_key.
    /// Uses a raw reqwest client because multipart is not supported by reqwest-middleware.
    pub async fn upload_file(
        &self,
        file_type: &str,
        file_name: &str,
        file_data: Vec<u8>,
    ) -> Result<String> {
        let url = "https://open.feishu.cn/open-apis/im/v1/files";
        let token = self.token_manager.get_tenant_access_token().await?;

        let part = reqwest::multipart::Part::bytes(file_data).file_name(file_name.to_string());

        let form = reqwest::multipart::Form::new()
            .text("file_type", file_type.to_string())
            .text("file_name", file_name.to_string())
            .part("file", part);

        let client = reqwest::Client::new();
        let resp = client
            .post(url)
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file to Feishu")?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .context("Failed to parse file upload response")?;
        if !status.is_success() {
            anyhow::bail!("Feishu file upload failed: {} - {}", status, body);
        }
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu file upload API error: code={}, msg={}", code, msg);
        }

        let file_key = body
            .get("data")
            .and_then(|d| d.get("file_key"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Feishu file upload response missing file_key"))?
            .to_string();
        Ok(file_key)
    }

    /// Send a file message to a Feishu chat.
    pub async fn send_file_message(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        file_key: &str,
    ) -> Result<String> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let msg_type = "file";
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", receive_id_type)])
            .json(&json!({
                "receive_id": receive_id,
                "msg_type": msg_type,
                "content": json!({"file_key": file_key}).to_string(),
            }))
            .send()
            .await
            .context("Failed to send Feishu file message")?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .context("Failed to parse file message response")?;
        if !status.is_success() {
            anyhow::bail!("Feishu send file message failed: {} - {}", status, body);
        }
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!(
                "Feishu send file message API error: code={}, msg={}",
                code,
                msg
            );
        }

        let message_id = body
            .get("data")
            .and_then(|d| d.get("message_id"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Feishu send file message response missing message_id"))?
            .to_string();
        Ok(message_id)
    }

    pub async fn list_chats(&self) -> Result<Vec<ChatItem>> {
        let url = "https://open.feishu.cn/open-apis/im/v1/chats";
        let resp = self
            .http_client
            .get(url)
            .send()
            .await
            .context("Failed to list chats")?;

        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .context("Failed to parse chat list response")?;

        if !status.is_success() {
            anyhow::bail!("Feishu list chats failed: {} - {}", status, body);
        }

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu API error: {} - {}", code, msg);
        }

        let items = body
            .get("data")
            .and_then(|d| d.get("items"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let chat_id = item.get("chat_id")?.as_str()?.to_string();
                        let name = item.get("name")?.as_str()?.to_string();
                        Some(ChatItem { chat_id, name })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(items)
    }

    // -----------------------------------------------------------------------
    // ACL
    // -----------------------------------------------------------------------

    pub(crate) fn is_allowed_sender(&self, open_id: &str) -> bool {
        if self.config.allow_from == "*" {
            return true;
        }
        self.config
            .allow_from
            .split(',')
            .any(|s| s.trim() == open_id)
    }

    // -----------------------------------------------------------------------
    // Reaction feedback
    // -----------------------------------------------------------------------

    async fn add_reaction(&self, message_id: &str, emoji_type: &str) -> Result<Option<String>> {
        let url = format!(
            "https://open.feishu.cn/open-apis/im/v1/messages/{}/reactions",
            message_id
        );
        let resp = self
            .http_client
            .post(&url)
            .json(&json!({
                "reaction_type": { "emoji_type": emoji_type }
            }))
            .send()
            .await
            .context("Failed to add reaction")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Add reaction failed: {} - {}", status, body);
        }

        let data: ReactionCreateResp = resp
            .json()
            .await
            .context("Failed to parse reaction response")?;
        if data.code != 0 {
            anyhow::bail!(
                "Feishu reaction API error: {} - {}",
                data.code,
                data.msg.unwrap_or_default()
            );
        }

        Ok(data.data.and_then(|d| d.reaction_id))
    }

    async fn remove_reaction(&self, message_id: &str, reaction_id: &str) -> Result<()> {
        let url = format!(
            "https://open.feishu.cn/open-apis/im/v1/messages/{}/reactions/{}",
            message_id, reaction_id
        );
        let resp = self
            .http_client
            .delete(&url)
            .send()
            .await
            .context("Failed to remove reaction")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Remove reaction failed: {} - {}", status, body);
        }

        Ok(())
    }

    pub(crate) async fn on_processing_start(&self, message_id: &str) {
        if message_id.is_empty() {
            return;
        }
        match self.add_reaction(message_id, REACTION_TYPING).await {
            Ok(Some(reaction_id)) => {
                self.pending_reactions
                    .insert(message_id.to_string(), reaction_id);
            }
            Ok(None) => {
                warn!(
                    "Add typing reaction returned no reaction_id for {}",
                    message_id
                );
            }
            Err(e) => {
                debug!("Failed to add typing reaction: {}", e);
            }
        }
    }

    pub(crate) async fn on_processing_complete(&self, message_id: &str, success: bool) {
        if message_id.is_empty() {
            return;
        }
        if let Some((_, reaction_id)) = self.pending_reactions.remove(message_id) {
            if let Err(e) = self.remove_reaction(message_id, &reaction_id).await {
                debug!("Failed to remove typing reaction: {}", e);
            }
        }
        if !success {
            if let Err(e) = self.add_reaction(message_id, REACTION_FAILURE).await {
                debug!("Failed to add failure reaction: {}", e);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Bot identity
    // -----------------------------------------------------------------------

    async fn fetch_bot_identity(&self) -> Result<Option<BotInfo>> {
        let url = "https://open.feishu.cn/open-apis/bot/v3/info";
        let resp = self
            .http_client
            .get(url)
            .send()
            .await
            .context("Failed to fetch bot identity")?;

        let data: BotInfoResp = resp.json().await.context("Failed to parse bot info")?;
        if data.code != 0 {
            anyhow::bail!(
                "Feishu bot info API error: {} - {}",
                data.code,
                data.msg.unwrap_or_default()
            );
        }

        Ok(data.bot)
    }

    async fn get_bot_open_id(&self) -> Option<String> {
        {
            let cached = self.bot_identity.read().await;
            if let Some(ref bot) = *cached {
                return bot.open_id.clone();
            }
        }
        match self.fetch_bot_identity().await {
            Ok(bot_info) => {
                let open_id = bot_info.as_ref().and_then(|b| b.open_id.clone());
                let mut cached = self.bot_identity.write().await;
                *cached = bot_info;
                open_id
            }
            Err(e) => {
                warn!("Failed to fetch bot identity: {}", e);
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions: text splitting, HTTP response, content extraction, frames
// ---------------------------------------------------------------------------

pub(crate) fn split_text_into_chunks(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_len = line.chars().count();
        if line_len > max_chars {
            if !current.is_empty() {
                chunks.push(current);
                current = String::new();
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
            chunks.push(current);
            current = line.to_string();
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

pub(crate) fn build_http_response(status: u16, body: &str) -> String {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, status_text, body.len(), body
    )
}

pub(crate) fn extract_post_content(content_str: &str) -> (String, Vec<String>) {
    let mut texts = Vec::new();
    let mut image_keys = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(content_str) {
        if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
            if !text.is_empty() {
                texts.push(text.to_string());
            }
        }
        if let Some(title) = v.get("title").and_then(|t| t.as_str()) {
            if !title.is_empty() {
                texts.push(title.to_string());
            }
        }
        if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
            for line in content {
                if let Some(line_arr) = line.as_array() {
                    for segment in line_arr {
                        if let Some(tag) = segment.get("tag").and_then(|t| t.as_str()) {
                            match tag {
                                "text" => {
                                    if let Some(text) = segment.get("text").and_then(|t| t.as_str())
                                    {
                                        texts.push(text.to_string());
                                    }
                                }
                                "a" => {
                                    if let Some(text) = segment.get("text").and_then(|t| t.as_str())
                                    {
                                        texts.push(text.to_string());
                                    }
                                }
                                "at" => {
                                    if let Some(name) =
                                        segment.get("user_name").and_then(|n| n.as_str())
                                    {
                                        texts.push(format!("@{}", name));
                                    }
                                }
                                "img" => {
                                    if let Some(key) =
                                        segment.get("image_key").and_then(|k| k.as_str())
                                    {
                                        image_keys.push(key.to_string());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    let text = if texts.is_empty() {
        content_str.to_string()
    } else {
        texts.join("\n")
    };
    (text, image_keys)
}

pub(crate) fn build_ping_frame(service_id: i32) -> Frame {
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: METHOD_CONTROL,
        headers: vec![Header {
            key: "type".to_string(),
            value: "ping".to_string(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

pub(crate) fn build_ack_frame(original_frame: &Frame) -> Frame {
    let mut ack_frame = original_frame.clone();

    if !ack_frame.headers.iter().any(|h| h.key == "biz_rt") {
        ack_frame.headers.push(Header {
            key: "biz_rt".to_string(),
            value: "0".to_string(),
        });
    }

    let ack = serde_json::json!({
        "code": 200,
        "headers": {},
        "data": null
    });
    ack_frame.payload = Some(ack.to_string().into_bytes());
    ack_frame
}
