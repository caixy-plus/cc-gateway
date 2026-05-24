#![allow(dead_code)]
pub(crate) mod interaction;
pub(crate) mod auth_middleware;
pub(crate) mod cards;
pub(crate) mod media;
pub(crate) mod webhook;
pub(crate) mod ws;
use anyhow::{Context, Result};
use dashmap::DashMap;
use reqwest;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, timeout, Duration as TokioDuration};
use tracing::{debug, info, warn};

use crate::config::model::{ClaudeConfig, FeishuConfig};
use crate::platform::proto::Frame;
use crate::session::channel_manager::{ActiveClaudeRuntime, GLOBAL_CHANNEL_SESSIONS};

/// Per-channel runtime for Feishu.
/// Each chat gets its own ChannelSession; active ClaudeSession is optional.
#[derive(Clone)]
struct FeishuChannelRuntime {
    channel_session: crate::session::channel_model::ChannelSession,
    active_claude: Option<ActiveClaudeRuntime>,
    receive_id_type: String,
    /// Ensures only one poll_claude_and_reply runs per chat at a time,
    /// preventing event-rx contention between concurrent messages.
    poll_lock: Arc<Mutex<()>>,
}

impl FeishuChannelRuntime {
    fn new(channel_session: crate::session::channel_model::ChannelSession, receive_id_type: String) -> Self {
        Self {
            channel_session,
            active_claude: None,
            receive_id_type,
            poll_lock: Arc::new(Mutex::new(())),
        }
    }
}

// ---------------------------------------------------------------------------
// Constants for Feishu pbbp2 WebSocket protocol
// ---------------------------------------------------------------------------

/// Method: CONTROL (ping/pong, connection management)
 pub(crate) const METHOD_CONTROL: i32 = 0;
/// Method: DATA (event/card payloads)
 pub(crate) const METHOD_DATA: i32 = 1;

/// Service: unknown / connection-level
 pub(crate) const SERVICE_SYSTEM: i32 = 0;
/// Service: IM message events
 pub(crate) const SERVICE_IM: i32 = 1;
/// Service: Card callback events
 pub(crate) const SERVICE_CARD: i32 = 2;

/// Default heartbeat interval (seconds). Feishu recommends 30s.
 pub(crate) const HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Max characters per Feishu text message (safety margin below 4096).
 pub(crate) const FEISHU_MAX_TEXT_CHARS: usize = 3500;
/// Delay between chunked message sends to avoid rate limits.
 pub(crate) const FEISHU_CHUNK_DELAY_MS: u64 = 300;

/// Reaction emoji type for "processing".
 pub(crate) const REACTION_TYPING: &str = "Typing";
/// Reaction emoji type for "failure".
 pub(crate) const REACTION_FAILURE: &str = "CrossMark";

// ---------------------------------------------------------------------------
// API response structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
 pub(crate) struct TenantAccessTokenResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(rename = "tenant_access_token")]
    tenant_access_token: String,
}


#[derive(Debug, Deserialize)]
pub struct ChatItem {
    pub(crate) chat_id: String,
    pub(crate) name: String,
}
#[derive(Debug, Deserialize)]
 pub(crate) struct WsEndpointResp {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<WsEndpointData>,
}

#[derive(Debug, Deserialize)]
 pub(crate) struct WsEndpointData {
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "ClientConfig")]
    client_config: Option<WsClientConfig>,
}

#[derive(Debug, Deserialize, Clone)]
 pub(crate) struct WsClientConfig {
    #[serde(rename = "ReconnectCount")]
    reconnect_count: i32,
    #[serde(rename = "ReconnectInterval")]
    reconnect_interval: i32,
    #[serde(rename = "ReconnectNonce")]
    reconnect_nonce: i32,
    #[serde(rename = "PingInterval")]
    ping_interval: i32,
}

#[derive(Debug, Deserialize)]
 pub(crate) struct ReactionCreateResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<ReactionData>,
}

#[derive(Debug, Deserialize)]
 pub(crate) struct ReactionData {
    #[serde(rename = "reaction_id")]
    reaction_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
 pub(crate) struct BotInfo {
    #[serde(rename = "open_id")]
    open_id: Option<String>,
}

#[derive(Debug, Deserialize)]
 pub(crate) struct BotInfoResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    bot: Option<BotInfo>,
}

// ---------------------------------------------------------------------------
// Event payload structs (serde)
// ---------------------------------------------------------------------------

/// Generic event wrapper delivered over WebSocket.
#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct EventWrapper {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    header: Option<EventHeader>,
    #[serde(default)]
    event: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct EventHeader {
    #[serde(rename = "event_id")]
    event_id: Option<String>,
    #[serde(rename = "event_type")]
    event_type: Option<String>,
    #[serde(rename = "create_time")]
    create_time: Option<String>,
    #[serde(rename = "token")]
    token: Option<String>,
    #[serde(rename = "app_id")]
    app_id: Option<String>,
    #[serde(rename = "tenant_key")]
    tenant_key: Option<String>,
}

/// IM message event body.
#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct ImMessageEvent {
    #[serde(default)]
    sender: Option<SenderInfo>,
    #[serde(default)]
    message: Option<MessageInfo>,
    #[serde(default)]
    mentions: Option<Vec<MentionEventInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct SenderInfo {
    #[serde(rename = "sender_id")]
    sender_id: Option<OpenIdInfo>,
    #[serde(rename = "sender_type")]
    sender_type: Option<String>,
    #[serde(rename = "tenant_key")]
    tenant_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct OpenIdInfo {
    #[serde(rename = "open_id")]
    open_id: Option<String>,
    #[serde(rename = "union_id")]
    union_id: Option<String>,
    #[serde(rename = "user_id")]
    user_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct MessageInfo {
    #[serde(rename = "message_id")]
    message_id: Option<String>,
    #[serde(rename = "root_id")]
    root_id: Option<String>,
    #[serde(rename = "parent_id")]
    parent_id: Option<String>,
    #[serde(rename = "create_time")]
    create_time: Option<String>,
    #[serde(rename = "chat_id")]
    chat_id: Option<String>,
    #[serde(rename = "chat_type")]
    chat_type: Option<String>,
    #[serde(rename = "message_type")]
    message_type: Option<String>,
    #[serde(rename = "content")]
    content: Option<String>,
    #[serde(default)]
    mentions: Option<Vec<MentionEventInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct MentionEventInfo {
    #[serde(rename = "key")]
    key: Option<String>,
    #[serde(rename = "id")]
    id: Option<OpenIdInfo>,
    #[serde(rename = "name")]
    name: Option<String>,
    #[serde(rename = "tenant_key")]
    tenant_key: Option<String>,
}

/// Parsed text content inside `message.content` JSON string.
#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct TextMessageContent {
    text: Option<String>,
}

/// Card action event body.
#[derive(Debug, Clone, Deserialize)]
 pub(crate) struct CardActionEvent {
    #[serde(rename = "open_message_id")]
    open_message_id: Option<String>,
    #[serde(rename = "open_id")]
    open_id: Option<String>,
    #[serde(rename = "tenant_key")]
    tenant_key: Option<String>,
    #[serde(default)]
    action: Option<Value>,
    #[serde(default)]
    #[serde(rename = "trigger_time")]
    trigger_time: Option<String>,
}

// ---------------------------------------------------------------------------
// Platform structs
// ---------------------------------------------------------------------------

/// Pending permission context stored for interactive card callbacks.
#[derive(Clone, Debug)]
pub struct PendingPermissionContext {
    pub request_id: String,
    pub tool_name: String,
    pub chat_id: String,
    pub sender_open_id: String,
    pub created_at: Instant,
}

/// Normalized Feishu message/event representation.
#[derive(Clone, Debug)]
pub struct NormalizedMessage {
    pub message_id: String,
    pub message_type: String,
    pub content: String,
    pub sender_open_id: String,
    pub sender_name: Option<String>,
    pub chat_id: Option<String>,
    pub chat_type: Option<String>,
    pub mentions: Vec<MentionInfo>,
    pub raw: Value,
    /// Resolved receive_id_type for Feishu API ("chat_id" or "open_id")
    pub receive_id_type: String,
    /// Resolved receive_id for Feishu API
    pub receive_id: String,
}

#[derive(Clone, Debug)]
pub struct MentionInfo {
    pub open_id: String,
    pub name: Option<String>,
    pub key: Option<String>,
}

/// Simple TTL deduplication cache using DashMap.
pub struct DedupCache {
    inner: DashMap<String, Instant>,
    ttl: Duration,
}

impl DedupCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: DashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub fn insert(&self, key: String) {
        self.inner.insert(key, Instant::now());
        self.cleanup();
    }

    pub fn contains(&self, key: &str) -> bool {
        if let Some(entry) = self.inner.get(key) {
            if entry.value().elapsed() < self.ttl {
                return true;
            }
        }
        false
    }

    fn cleanup(&self) {
        let now = Instant::now();
        self.inner.retain(|_, v| now.duration_since(*v) < self.ttl);
    }
}

/// Sliding-window rate limiter keyed by IP.
pub struct RateLimiter {
    inner: DashMap<String, Vec<Instant>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            inner: DashMap::new(),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn check(&self, key: &str) -> bool {
        let now = Instant::now();
        let mut entry = self.inner.entry(key.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < self.window);
        if entry.len() >= self.max_requests {
            false
        } else {
            entry.push(now);
            true
        }
    }
}

/// Tracks consecutive error responses per IP.
pub struct AnomalyTracker {
    inner: DashMap<String, (u32, Instant)>,
    threshold: u32,
    ttl: Duration,
}

impl AnomalyTracker {
    pub fn new(threshold: u32, ttl_secs: u64) -> Self {
        Self {
            inner: DashMap::new(),
            threshold,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub fn record(&self, key: &str, status: u16) {
        let now = Instant::now();
        if status < 400 {
            self.inner.remove(key);
            return;
        }
        if let Some(mut entry) = self.inner.get_mut(key) {
            let (count, first_seen) = *entry;
            if now.duration_since(first_seen) > self.ttl {
                *entry = (1, now);
            } else {
                entry.0 = count + 1;
                let new_count = entry.0;
                if new_count % self.threshold == 0 {
                    warn!(
                        "[Feishu] Webhook anomaly: {} consecutive error responses from {} over last {}s",
                        new_count,
                        key,
                        now.duration_since(first_seen).as_secs()
                    );
                }
            }
        } else {
            self.inner.insert(key.to_string(), (1, now));
        }
    }
}

#[derive(Clone)]
pub struct FeishuPlatform {
    config: FeishuConfig,
    default_dir: String,
    claude_config: ClaudeConfig,
    show_thinking: Arc<AtomicBool>,
    http_client: ClientWithMiddleware,
    dedup_cache: Arc<DedupCache>,
    pending_permissions: Arc<DashMap<String, PendingPermissionContext>>,
    interaction_store: Arc<interaction::InteractionStore>,
    pub(crate) token_manager: auth_middleware::TokenManager,
    /// message_id -> reaction_id for in-progress reactions.
    pending_reactions: Arc<DashMap<String, String>>,
    /// Cached bot identity (open_id) for mention matching.
    bot_identity: Arc<RwLock<Option<BotInfo>>>,
    /// Webhook rate limiter.
    rate_limiter: Arc<RateLimiter>,
    /// Webhook anomaly tracker.
    anomaly_tracker: Arc<AnomalyTracker>,
    /// Per-chat channels: each chat gets its own ChannelSession.
    channels: Arc<DashMap<String, FeishuChannelRuntime>>,
}

impl FeishuPlatform {
    pub fn new(
        config: FeishuConfig,
        default_dir: &str,
        claude_config: ClaudeConfig,
        show_thinking: bool,
    ) -> Self {
        let token_manager = auth_middleware::TokenManager::new(config.clone());
        let http_client = reqwest_middleware::ClientBuilder::new(reqwest::Client::new())
            .with(auth_middleware::FeishuAuthMiddleware::new(token_manager.clone()))
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

    async fn get_channel(&self, chat_id: &str, receive_id_type: &str) -> FeishuChannelRuntime {
        if let Some(runtime) = self.channels.get(chat_id) {
            return runtime.clone();
        }
        let channel = GLOBAL_CHANNEL_SESSIONS.get_or_create_platform_channel(
            "feishu",
            chat_id,
            &self.default_dir,
        ).await;
        let runtime = FeishuChannelRuntime::new(channel, receive_id_type.to_string());
        self.channels.insert(chat_id.to_string(), runtime.clone());
        runtime
    }

    fn spawn_deliver_listener(&self) {
        let platform = self.clone();
        crate::platform::spawn_deliver_listener("feishu", move |channel_id, text| {
            let platform = platform.clone();
            tokio::spawn(async move {
                let receive_id_type = if let Some(runtime) = platform.channels.get(&channel_id) {
                    runtime.receive_id_type.clone()
                } else {
                    "chat_id".to_string()
                };
                let _ = platform.send_text_message(&receive_id_type, &channel_id, &text).await;
            });
        });
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Feishu platform...");
        self.spawn_deliver_listener();

        let (ws_url, client_config) = self.get_ws_endpoint().await?;
        info!("Feishu WebSocket endpoint: {}", ws_url);

        self.run_websocket(&ws_url, client_config).await
    }

    /// Start a minimal HTTP webhook server (no extra framework deps).
    pub async fn run_webhook(&self) -> Result<()> {
        let bind_addr = &self.config.webhook_bind;
        let listener = tokio::net::TcpListener::bind(bind_addr).await
            .with_context(|| format!("Failed to bind webhook server to {}", bind_addr))?;
        info!("Feishu webhook server listening on {}", bind_addr);

        loop {
            let (mut stream, addr) = listener.accept().await
                .context("Failed to accept webhook connection")?;
            let platform = self.clone();
            let ip = addr.ip().to_string();
            if !platform.rate_limiter.check(&ip) {
                warn!("[Feishu] Webhook rate limit exceeded for {}", ip);
                platform.anomaly_tracker.record(&ip, 429);
                let _ = {
                    use tokio::io::AsyncWriteExt;
                    stream.write_all(
                        build_http_response(429, r#"{"code":1,"msg":"too many requests"}"#).as_bytes()
                    ).await
                };
                continue;
            }
            tokio::spawn(async move {
                if let Err(e) = platform.handle_webhook_connection(stream, addr).await {
                    debug!("Webhook connection from {} error: {}", addr, e);
                }
            });
        }
    }

    async fn handle_webhook_connection(
        &self,
        stream: tokio::net::TcpStream,
        addr: std::net::SocketAddr,
    ) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut request_line = String::new();

        // Read request line
        if reader.read_line(&mut request_line).await? == 0 {
            return Ok(());
        }
        let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
        if parts.len() < 2 {
            return Ok(());
        }
        let method = parts[0].to_uppercase();
        let path = parts[1].to_string();

        // Read headers
        let mut headers = std::collections::HashMap::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            if line.trim().is_empty() {
                break;
            }
            if let Some((k, v)) = line.trim().split_once(": ") {
                headers.insert(k.to_lowercase(), v.to_string());
            }
        }

        // Read body
        let content_length = headers
            .get("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).await?;
        }

        debug!("Webhook {} {} from {} (body={} bytes)", method, path, addr, content_length);

        let ip = addr.ip().to_string();
        let (status, body_str) = match (method.as_str(), path.as_str()) {
            ("POST", "/webhook") => {
                match serde_json::from_slice::<Value>(&body) {
                    Ok(json) => {
                        if json.get("challenge").is_some() {
                            match self.verify_challenge(&json) {
                                Ok(resp) => (200, resp.to_string()),
                                Err(e) => (400, json!({"error": e.to_string()}).to_string()),
                            }
                        } else {
                            let event_type = json
                                .get("header")
                                .and_then(|h| h.get("event_type"))
                                .and_then(|v| v.as_str());
                            match event_type {
                                Some("im.message.receive_v1") => {
                                    let _ = self.handle_event(&json).await;
                                    (200, r#"{"code":0}"#.to_string())
                                }
                                Some(other) => {
                                    debug!("Unhandled webhook event type: {}", other);
                                    (200, r#"{"code":0}"#.to_string())
                                }
                                None => {
                                    warn!("Webhook event missing event_type");
                                    (200, r#"{"code":0}"#.to_string())
                                }
                            }
                        }
                    }
                    Err(_) => (400, r#"{"code":1,"msg":"invalid json"}"#.to_string()),
                }
            }
            _ => (404, r#"{"code":1,"msg":"not found"}"#.to_string()),
        };

        self.anomaly_tracker.record(&ip, status);
        writer.write_all(build_http_response(status, &body_str).as_bytes()).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // HTTP helpers
    // -----------------------------------------------------------------------

    pub(crate) async fn get_tenant_access_token(&self) -> Result<String> {
        self.token_manager.get_tenant_access_token().await
    }

    async fn get_ws_endpoint(&self) -> Result<(String, WsClientConfig)> {
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
                .send()
        )
        .await
        .map_err(|_| anyhow::anyhow!("Request WebSocket endpoint timeout (10s)"))?
        .context("Failed to request WebSocket endpoint")?;

        let body_text = resp.text().await.context("Failed to read WebSocket endpoint response body")?;
        debug!("Feishu WS endpoint raw response: {}", body_text);
        let data: WsEndpointResp = serde_json::from_str(&body_text)
            .with_context(|| format!("Failed to parse WebSocket endpoint response: {}", body_text))?;

        if data.code != 0 {
            anyhow::bail!(
                "Feishu WS endpoint error: {} - {}",
                data.code,
                data.msg
            );
        }

        let endpoint = data.data.ok_or_else(|| anyhow::anyhow!("Feishu WS endpoint response missing data"))?;
        let ws_url = endpoint.url.ok_or_else(|| anyhow::anyhow!("Feishu WS endpoint response missing URL"))?;
        let client_config = endpoint.client_config.unwrap_or(WsClientConfig {
            reconnect_count: 10,
            reconnect_interval: 5,
            reconnect_nonce: 5,
            ping_interval: 30,
        });
        Ok((ws_url, client_config))
    }

    /// Send a plain text message to a Feishu chat. Long messages are split into chunks
    /// to avoid triggering Feishu rate limits.
    pub async fn send_text_message(&self, receive_id_type: &str, receive_id: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let chunks = split_text_into_chunks(text, FEISHU_MAX_TEXT_CHARS);
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                sleep(TokioDuration::from_millis(FEISHU_CHUNK_DELAY_MS)).await;
            }
            self.send_text_message_raw(receive_id_type, receive_id, chunk).await?;
        }
        Ok(())
    }

    async fn send_text_message_raw(&self, receive_id_type: &str, receive_id: &str, text: &str) -> Result<()> {
        match self.send_text_message_raw_inner(receive_id_type, receive_id, text).await {
            Ok(()) => Ok(()),
            Err(e) if auth_middleware::TokenManager::is_auth_error(&e) => {
                self.token_manager.invalidate_token_cache().await;
                self.send_text_message_raw_inner(receive_id_type, receive_id, text).await
            }
            Err(e) => Err(e),
        }
    }

    async fn send_text_message_raw_inner(&self, receive_id_type: &str, receive_id: &str, text: &str) -> Result<()> {
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
            anyhow::bail!("Feishu send text message failed: {} - {}", status, body_text);
        }
        let body: Value = serde_json::from_str(&body_text)
            .unwrap_or_else(|_| serde_json::json!({"code": -1}));
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown");
            anyhow::bail!("Feishu API error (send text): code={}, msg={}", code, msg);
        }

        Ok(())
    }

    /// Send a rich text (post) message to a Feishu chat.
    pub async fn send_post_message(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        content: &Value,
    ) -> Result<()> {
        match self.send_post_message_inner(receive_id_type, receive_id, content).await {
            Ok(()) => Ok(()),
            Err(e) if auth_middleware::TokenManager::is_auth_error(&e) => {
                self.token_manager.invalidate_token_cache().await;
                self.send_post_message_inner(receive_id_type, receive_id, content).await
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

    /// Send an interactive card message to a Feishu chat.
    pub async fn send_interactive_card(
        &self,
        receive_id_type: &str,
        receive_id: &str,
        card_json: &Value,
    ) -> Result<()> {
        if receive_id.is_empty() {
            anyhow::bail!("Cannot send card: receive_id is empty");
        }
        match self.send_interactive_card_inner(receive_id_type, receive_id, card_json).await {
            Ok(()) => Ok(()),
            Err(e) if auth_middleware::TokenManager::is_auth_error(&e) => {
                self.token_manager.invalidate_token_cache().await;
                self.send_interactive_card_inner(receive_id_type, receive_id, card_json).await
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
        debug!("Sending interactive card to receive_id_type={} receive_id={}, body={}", receive_id_type, receive_id, request_body);
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
        let body: Value = serde_json::from_str(&body_text)
            .unwrap_or_else(|_| serde_json::json!({"code": -1}));
        if !status.is_success() {
            anyhow::bail!(
                "Feishu send interactive card failed: {} - {}",
                status,
                body_text
            );
        }
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown");
            anyhow::bail!("Feishu API error (send card): code={}, msg={}", code, msg);
        }

        Ok(())
    }

    /// Gracefully shutdown all per-chat Claude sessions.
    /// Sends a "session exited" message to each chat, then gives each session
    /// 500ms to exit; if it doesn't, the timeout cancels the future and
    /// tokio::process::Child's Drop impl sends SIGKILL.
    pub async fn shutdown_all_sessions(&self) {
        for entry in self.channels.iter() {
            let chat_id = entry.key().clone();
            let runtime = entry.value().clone();
            drop(entry);

            // Notify the chat that the bot is shutting down
            let _ = self
                .send_text_message(
                    &runtime.receive_id_type,
                    &chat_id,
                    "机器人正在关闭，会话已退出",
                )
                .await;

            if let Some(ref active) = runtime.active_claude {
                let ctrl = active.controller.lock().await;
                match tokio::time::timeout(
                    tokio::time::Duration::from_millis(500),
                    ctrl.stop_session(),
                )
                .await
                {
                    Ok(Ok(())) => info!("[Feishu] Session {} stopped gracefully", chat_id),
                    Ok(Err(e)) => warn!("[Feishu] Session {} stop error: {}", chat_id, e),
                    Err(_) => warn!("[Feishu] Session {} stop timed out, killing", chat_id),
                }
            }
        }
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
        let body: Value = resp.json().await.context("Failed to parse chat list response")?;

        if !status.is_success() {
            anyhow::bail!("Feishu list chats failed: {} - {}", status, body);
        }

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown");
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

    fn is_allowed_sender(&self, open_id: &str) -> bool {
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

    async fn add_reaction(
        &self,
        message_id: &str,
        emoji_type: &str,
    ) -> Result<Option<String>> {
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

    async fn remove_reaction(
        &self,
        message_id: &str,
        reaction_id: &str,
    ) -> Result<()> {
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

    async fn on_processing_start(&self, message_id: &str) {
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

    async fn on_processing_complete(&self, message_id: &str, success: bool) {
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

        let data: BotInfoResp = resp
            .json()
            .await
            .context("Failed to parse bot info")?;
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
// Frame builders
// ---------------------------------------------------------------------------

/// Split long text into chunks at line boundaries where possible,
/// falling back to character boundaries for very long lines.
pub(crate) fn split_text_into_chunks(text: &str, max_chars: usize) -> Vec<String> {
    if text.len() <= max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_len = line.len();
        if line_len > max_chars {
            // Flush current chunk if any
            if !current.is_empty() {
                chunks.push(current);
                current = String::new();
            }
            // Split the long line into character chunks
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
        } else if !current.is_empty() && current.len() + line_len + 1 > max_chars {
            // Flush current chunk and start new one with this line
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
        status,
        status_text,
        body.len(),
        body
    )
}

/// Extract text and image keys from a Feishu post message content JSON.
/// Returns (text, list_of_image_keys).
pub(crate) fn extract_post_content(content_str: &str) -> (String, Vec<String>) {
    let mut texts = Vec::new();
    let mut image_keys = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(content_str) {
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
                                    if let Some(text) = segment.get("text").and_then(|t| t.as_str()) {
                                        texts.push(text.to_string());
                                    }
                                }
                                "a" => {
                                    if let Some(text) = segment.get("text").and_then(|t| t.as_str()) {
                                        texts.push(text.to_string());
                                    }
                                }
                                "at" => {
                                    if let Some(name) = segment.get("user_name").and_then(|n| n.as_str()) {
                                        texts.push(format!("@{}", name));
                                    }
                                }
                                "img" => {
                                    if let Some(key) = segment.get("image_key").and_then(|k| k.as_str()) {
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
    use crate::platform::proto::Header;
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

fn build_pong_frame(service_id: i32) -> Frame {
    use crate::platform::proto::Header;
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: METHOD_CONTROL,
        headers: vec![Header {
            key: "type".to_string(),
            value: "pong".to_string(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

/// Build an ACK frame for a received data event.
/// Matches official Feishu/Lark SDK behavior: keep the original DATA method (1)
/// and respond with a Response payload so the server knows we processed it.
 pub(crate) fn build_ack_frame(original_frame: &Frame) -> Frame {
    use crate::platform::proto::Header;
    let mut ack_frame = original_frame.clone();

    // IMPORTANT: Do NOT change method to CONTROL (0).
    // Official SDKs (Go, Python, Java) keep the original DATA method (1) for ACKs.
    // Changing it to CONTROL causes the server to think we never ACKed,
    // so it stops pushing events after the initial connection.

    // Add biz_rt header (business processing time, 0ms as placeholder)
    if !ack_frame.headers.iter().any(|h| h.key == "biz_rt") {
        ack_frame.headers.push(Header {
            key: "biz_rt".to_string(),
            value: "0".to_string(),
        });
    }

    // Match official SDK Response format: {code, headers, data}
    let ack = serde_json::json!({
        "code": 200,
        "headers": {},
        "data": null
    });
    ack_frame.payload = Some(ack.to_string().into_bytes());
    ack_frame
}

// ---------------------------------------------------------------------------
// Platform trait implementation
// ---------------------------------------------------------------------------

use crate::platform::Platform;
use async_trait::async_trait;

#[async_trait]
impl Platform for FeishuPlatform {
    async fn run(&self) -> Result<()> {
        if self.config.mode == "webhook" {
            self.run_webhook().await
        } else {
            self.run().await
        }
    }

    async fn shutdown(&self) {
        self.shutdown_all_sessions().await;
    }
}
