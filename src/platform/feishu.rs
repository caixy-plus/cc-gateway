#![allow(dead_code)]
use anyhow::{Context, Result};
use bytes::BytesMut;
use dashmap::DashMap;
use futures::{SinkExt, StreamExt};
use reqwest;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, timeout, Duration as TokioDuration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tracing::{debug, info, warn};

use crate::claude::controller::{ClaudeController, ControllerEvent};
use crate::claude::event_formatter::EventAccumulator;
use crate::command::router::CommandRouter;
use crate::config::model::FeishuConfig;
use crate::platform::proto::Frame;
use crate::{t, t_fmt};

// ---------------------------------------------------------------------------
// Constants for Feishu pbbp2 WebSocket protocol
// ---------------------------------------------------------------------------

/// Method: CONTROL (ping/pong, connection management)
const METHOD_CONTROL: i32 = 0;
/// Method: DATA (event/card payloads)
const METHOD_DATA: i32 = 1;

/// Service: unknown / connection-level
const SERVICE_SYSTEM: i32 = 0;
/// Service: IM message events
const SERVICE_IM: i32 = 1;
/// Service: Card callback events
const SERVICE_CARD: i32 = 2;

/// Default heartbeat interval (seconds). Feishu recommends 30s.
const HEARTBEAT_INTERVAL_SECS: u64 = 30;

/// Max characters per Feishu text message (safety margin below 4096).
const FEISHU_MAX_TEXT_CHARS: usize = 3500;
/// Delay between chunked message sends to avoid rate limits.
const FEISHU_CHUNK_DELAY_MS: u64 = 300;

/// Reaction emoji type for "processing".
const REACTION_TYPING: &str = "Typing";
/// Reaction emoji type for "failure".
const REACTION_FAILURE: &str = "CrossMark";

// ---------------------------------------------------------------------------
// API response structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct TenantAccessTokenResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(rename = "tenant_access_token")]
    tenant_access_token: String,
}


#[derive(Debug, Deserialize)]
pub struct ChatItem {
    chat_id: String,
    name: String,
}
#[derive(Debug, Deserialize)]
struct WsEndpointResp {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    msg: String,
    #[serde(default)]
    data: Option<WsEndpointData>,
}

#[derive(Debug, Deserialize)]
struct WsEndpointData {
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "ClientConfig")]
    client_config: Option<WsClientConfig>,
}

#[derive(Debug, Deserialize, Clone)]
struct WsClientConfig {
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
struct ReactionCreateResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<ReactionData>,
}

#[derive(Debug, Deserialize)]
struct ReactionData {
    #[serde(rename = "reaction_id")]
    reaction_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct BotInfo {
    #[serde(rename = "open_id")]
    open_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BotInfoResp {
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
struct EventWrapper {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    header: Option<EventHeader>,
    #[serde(default)]
    event: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct EventHeader {
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
struct ImMessageEvent {
    #[serde(default)]
    sender: Option<SenderInfo>,
    #[serde(default)]
    message: Option<MessageInfo>,
    #[serde(default)]
    mentions: Option<Vec<MentionEventInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SenderInfo {
    #[serde(rename = "sender_id")]
    sender_id: Option<OpenIdInfo>,
    #[serde(rename = "sender_type")]
    sender_type: Option<String>,
    #[serde(rename = "tenant_key")]
    tenant_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenIdInfo {
    #[serde(rename = "open_id")]
    open_id: Option<String>,
    #[serde(rename = "union_id")]
    union_id: Option<String>,
    #[serde(rename = "user_id")]
    user_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MessageInfo {
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
struct MentionEventInfo {
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
struct TextMessageContent {
    text: Option<String>,
}

/// Card action event body.
#[derive(Debug, Clone, Deserialize)]
struct CardActionEvent {
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
    router: Arc<CommandRouter>,
    controller: Arc<Mutex<ClaudeController>>,
    http_client: reqwest::Client,
    dedup_cache: Arc<DedupCache>,
    pending_permissions: Arc<DashMap<String, PendingPermissionContext>>,
    cached_token: Arc<RwLock<Option<String>>>,
    token_fetched_at: Arc<RwLock<Option<Instant>>>,
    /// message_id -> reaction_id for in-progress reactions.
    pending_reactions: Arc<DashMap<String, String>>,
    /// Cached bot identity (open_id) for mention matching.
    bot_identity: Arc<RwLock<Option<BotInfo>>>,
    /// Webhook rate limiter.
    rate_limiter: Arc<RateLimiter>,
    /// Webhook anomaly tracker.
    anomaly_tracker: Arc<AnomalyTracker>,
}

impl FeishuPlatform {
    pub fn new(
        config: FeishuConfig,
        default_dir: &str,
        router: Arc<CommandRouter>,
        controller: Arc<Mutex<ClaudeController>>,
    ) -> Self {
        Self {
            config,
            default_dir: default_dir.to_string(),
            router,
            controller,
            http_client: reqwest::Client::new(),
            dedup_cache: Arc::new(DedupCache::new(300)),
            pending_permissions: Arc::new(DashMap::new()),
            cached_token: Arc::new(RwLock::new(None)),
            token_fetched_at: Arc::new(RwLock::new(None)),
            pending_reactions: Arc::new(DashMap::new()),
            bot_identity: Arc::new(RwLock::new(None)),
            rate_limiter: Arc::new(RateLimiter::new(60, 60)),
            anomaly_tracker: Arc::new(AnomalyTracker::new(25, 21600)),
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Feishu platform...");

        let (ws_url, client_config) = self.get_ws_endpoint().await?;
        info!("Feishu WebSocket endpoint: {}", ws_url);

        let token = self.get_tenant_access_token().await?;
        info!("Feishu tenant access token obtained");

        self.run_websocket(&ws_url, &token, client_config).await
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
                            match self.handle_webhook_event(&json).await {
                                Ok(Some(msg)) => {
                                    let token = match self.get_tenant_access_token().await {
                                        Ok(t) => t,
                                        Err(e) => {
                                            let resp = build_http_response(500, &json!({"error": e.to_string()}).to_string());
                                            writer.write_all(resp.as_bytes()).await?;
                                            self.anomaly_tracker.record(&ip, 500);
                                            return Ok(());
                                        }
                                    };
                                    let _ = self.route_message(msg, &token).await;
                                    (200, r#"{"code":0}"#.to_string())
                                }
                                Ok(None) => (200, r#"{"code":0}"#.to_string()),
                                Err(e) => (500, json!({"error": e.to_string()}).to_string()),
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

    async fn get_tenant_access_token(&self) -> Result<String> {
        // Check cache
        {
            let cached = self.cached_token.read().await;
            let fetched_at = self.token_fetched_at.read().await;
            if let (Some(token), Some(instant)) = (cached.as_ref(), fetched_at.as_ref()) {
                if instant.elapsed().as_secs() < 5400 {
                    return Ok(token.clone());
                }
            }
        }

        // Cache miss or expired — fetch and store
        let token = self.fetch_tenant_access_token().await?;
        let mut cached = self.cached_token.write().await;
        let mut fetched_at = self.token_fetched_at.write().await;
        *cached = Some(token.clone());
        *fetched_at = Some(Instant::now());
        Ok(token)
    }

    pub async fn refresh_token(&self) -> Result<String> {
        let token = self.fetch_tenant_access_token().await?;
        let mut cached = self.cached_token.write().await;
        let mut fetched_at = self.token_fetched_at.write().await;
        *cached = Some(token.clone());
        *fetched_at = Some(Instant::now());
        Ok(token)
    }

    async fn fetch_tenant_access_token(&self) -> Result<String> {
        let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
        let resp = self
            .http_client
            .post(url)
            .json(&json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .context("Failed to request tenant access token")?;

        let data: TenantAccessTokenResp = resp
            .json()
            .await
            .context("Failed to parse tenant access token response")?;

        if data.code != 0 {
            anyhow::bail!(
                "Feishu API error: {} - {}",
                data.code,
                data.msg.unwrap_or_default()
            );
        }

        Ok(data.tenant_access_token)
    }

    async fn get_ws_endpoint(&self) -> Result<(String, WsClientConfig)> {
        let url = "https://open.feishu.cn/callback/ws/endpoint";
        let resp = self
            .http_client
            .post(url)
            .header("locale", "zh")
            .json(&serde_json::json!({
                "AppID": &self.config.app_id,
                "AppSecret": &self.config.app_secret,
            }))
            .send()
            .await
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
    pub async fn send_text_message(&self, token: &str, receive_id_type: &str, receive_id: &str, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        let chunks = split_text_into_chunks(text, FEISHU_MAX_TEXT_CHARS);
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                sleep(TokioDuration::from_millis(FEISHU_CHUNK_DELAY_MS)).await;
            }
            self.send_text_message_raw(token, receive_id_type, receive_id, chunk).await?;
        }
        Ok(())
    }

    async fn send_text_message_raw(&self, token: &str, receive_id_type: &str, receive_id: &str, text: &str) -> Result<()> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", receive_id_type)])
            .header("Authorization", format!("Bearer {}", token))
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
        token: &str,
        receive_id_type: &str,
        receive_id: &str,
        content: &Value,
    ) -> Result<()> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", receive_id_type)])
            .header("Authorization", format!("Bearer {}", token))
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
        token: &str,
        receive_id_type: &str,
        receive_id: &str,
        card_json: &Value,
    ) -> Result<()> {
        if receive_id.is_empty() {
            anyhow::bail!("Cannot send card: receive_id is empty");
        }
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
            .header("Authorization", format!("Bearer {}", token))
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

    // -----------------------------------------------------------------------
    // Media download
    // -----------------------------------------------------------------------

    /// Download a message resource (image/file/audio) from Feishu and cache it locally.
    async fn download_message_resource(
        &self,
        token: &str,
        message_id: &str,
        file_key: &str,
        resource_type: &str,
    ) -> Result<Option<(String, String)>> {
        let url = format!(
            "https://open.feishu.cn/open-apis/im/v1/messages/{}/resources/{}",
            message_id, file_key
        );
        let resp = self
            .http_client
            .get(&url)
            .query(&[("type", resource_type)])
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .with_context(|| format!("Failed to download {} resource {}", resource_type, file_key))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!(
                "Feishu resource download failed: {} - {}",
                status,
                body
            );
        }

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("application/octet-stream")
            .to_string();

        let bytes = resp.bytes().await.context("Failed to read resource bytes")?;

        let cache_dir = dirs::home_dir()
            .map(|p| p.join(".cc-gateway").join("media"))
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp/cc-gateway/media"));
        std::fs::create_dir_all(&cache_dir)
            .with_context(|| format!("Failed to create media cache dir {:?}", cache_dir))?;

        let ext = match content_type.as_str() {
            "image/jpeg" | "image/jpg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            "audio/mpeg" | "audio/mp3" => "mp3",
            "audio/ogg" => "ogg",
            "audio/wav" => "wav",
            "audio/mp4" => "m4a",
            "video/mp4" => "mp4",
            "text/plain" => "txt",
            "text/markdown" => "md",
            "application/pdf" => "pdf",
            _ => "bin",
        };
        let filename = format!("{}_{}.{}", resource_type, file_key, ext);
        let path = cache_dir.join(&filename);
        tokio::fs::write(&path, &bytes)
            .await
            .with_context(|| format!("Failed to write media file {:?}", path))?;

        let path_str = path.to_string_lossy().to_string();
        info!(
            "[Feishu] Cached {} resource at {} ({} bytes, {})",
            resource_type,
            path_str,
            bytes.len(),
            content_type
        );
        Ok(Some((path_str, content_type)))
    }

    /// Build an interactive approval card for Claude tool permission requests.
    /// Returns a Feishu card protocol v2 JSON object.
    pub fn build_permission_card(
        &self,
        request_id: &str,
        tool_name: &str,
        tool_input: Option<&Value>,
    ) -> Value {
        let input_preview = tool_input
            .and_then(|v| serde_json::to_string_pretty(v).ok())
            .unwrap_or_else(|| "{}".to_string());
        // Truncate if too long
        let input_preview = if input_preview.len() > 500 {
            format!("{}...", &input_preview[..500])
        } else {
            input_preview
        };

        json!({
            "schema": "2.0",
            "config": {
                "style": {
                    "text_size": {
                        "level1": 17,
                        "level2": 16,
                        "level3": 14
                    }
                }
            },
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": t!("feishu.permission_title")
                },
                "subtitle": {
                    "tag": "plain_text",
                    "content": t_fmt!("feishu.permission_subtitle", NAME = tool_name)
                },
                "template": "indigo"
            },
            "body": {
                "elements": [
                    {
                        "tag": "div",
                        "text": {
                            "tag": "lark_md",
                            "content": t_fmt!("feishu.request_id_label", ID = request_id)
                        }
                    },
                    {
                        "tag": "div",
                        "text": {
                            "tag": "lark_md",
                            "content": t_fmt!("feishu.tool_input_label", INPUT = input_preview)
                        }
                    },
                    {
                        "tag": "hr"
                    },
                    {
                        "tag": "action",
                        "layout": "default",
                        "actions": [
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.approve_once")
                                },
                                "type": "primary",
                                "value": {
                                    "action": "approve_once",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.approve_session")
                                },
                                "type": "primary",
                                "value": {
                                    "action": "approve_session",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.approve_always")
                                },
                                "type": "primary",
                                "value": {
                                    "action": "approve_always",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            },
                            {
                                "tag": "button",
                                "text": {
                                    "tag": "plain_text",
                                    "content": t!("feishu.deny")
                                },
                                "type": "danger",
                                "value": {
                                    "action": "deny",
                                    "request_id": request_id,
                                    "tool_name": tool_name
                                }
                            }
                        ]
                    }
                ]
            }
        })
    }

    /// Build an interactive directory selection card.
    /// Feishu card schema v2: buttons are placed directly in body.elements.
    pub fn build_dir_select_card(&self, dirs: &[(String, String)], receive_id_type: &str, receive_id: &str) -> Value {
        let mut elements: Vec<Value> = Vec::new();
        elements.push(json!({
            "tag": "div",
            "text": {
                "tag": "lark_md",
                "content": t!("feishu.choose_dir")
            }
        }));

        const MAX_DIRS: usize = 40;
        for (name, path) in dirs.iter().take(MAX_DIRS) {
            elements.push(json!({
                "tag": "button",
                "text": {
                    "tag": "plain_text",
                    "content": name
                },
                "type": "primary",
                "behaviors": [
                    {
                        "type": "callback",
                        "value": {
                            "cmd": "cd",
                            "path": path,
                            "chat_id": receive_id,
                            "receive_id_type": receive_id_type
                        }
                    }
                ]
            }));
        }

        if dirs.len() > MAX_DIRS {
            let remaining = dirs.len() - MAX_DIRS;
            elements.push(json!({
                "tag": "div",
                "text": {
                    "tag": "lark_md",
                    "content": t_fmt!("feishu.more_dirs", COUNT = remaining)
                }
            }));
        }

        json!({
            "schema": "2.0",
            "header": {
                "title": {
                    "tag": "plain_text",
                    "content": t!("feishu.select_dir_title")
                },
                "template": "indigo"
            },
            "body": {
                "elements": elements
            }
        })
    }

    /// List directory names under the given path.
    /// Store a pending permission context so card callbacks can be matched to requests.
    pub fn store_pending_permission(&self, ctx: PendingPermissionContext) {
        self.pending_permissions
            .insert(ctx.request_id.clone(), ctx);
    }

    /// Retrieve and remove a pending permission context by request_id.
    pub fn take_pending_permission(&self, request_id: &str) -> Option<PendingPermissionContext> {
        self.pending_permissions.remove(request_id).map(|(_, v)| v)
    }

    /// Clean up expired pending permissions (older than 10 minutes).
    pub fn cleanup_pending_permissions(&self) {
        let now = Instant::now();
        let max_age = Duration::from_secs(600);
        self.pending_permissions
            .retain(|_, v| now.duration_since(v.created_at) < max_age);
    }

    /// Normalize a raw Feishu event JSON into a structured message.
    pub fn normalize_message(&self, event_json: &Value) -> Option<NormalizedMessage> {
        let event = event_json.get("event")?;
        let message = event.get("message")?;

        let message_id = message
            .get("message_id")?
            .as_str()?
            .to_string();

        // Deduplicate by message_id
        if self.dedup_cache.contains(&message_id) {
            return None;
        }
        self.dedup_cache.insert(message_id.clone());

        let message_type = message
            .get("message_type")?
            .as_str()?
            .to_string();

        let content = message
            .get("content")?
            .as_str()?
            .to_string();

        let sender = event.get("sender")?;
        let sender_id = sender.get("sender_id")?;
        let sender_open_id = sender_id
            .get("open_id")?
            .as_str()?
            .to_string();
        let sender_name = sender
            .get("sender_type")?
            .as_str()
            .map(|s| s.to_string());

        let chat_id = message
            .get("chat_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let chat_type = message
            .get("chat_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        // Extract mentions
        let mut mentions = Vec::new();
        if let Some(mentions_arr) = message.get("mentions").and_then(|v| v.as_array()) {
            for m in mentions_arr {
                let open_id = m
                    .get("id")
                    .and_then(|v| v.get("open_id"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let name = m
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let key = m
                    .get("key")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if let Some(oid) = open_id {
                    mentions.push(MentionInfo {
                        open_id: oid,
                        name,
                        key,
                    });
                }
            }
        }

        let (receive_id_type, receive_id) = if chat_type.as_deref() == Some("p2p") {
            ("open_id".to_string(), sender_open_id.clone())
        } else {
            ("chat_id".to_string(), chat_id.clone().unwrap_or_default())
        };

        Some(NormalizedMessage {
            message_id,
            message_type,
            content,
            sender_open_id,
            sender_name,
            chat_id,
            chat_type,
            mentions,
            raw: event_json.clone(),
            receive_id_type,
            receive_id,
        })
    }

    /// Handle an incoming Feishu event: normalize, route, and respond.
    pub async fn handle_event(
        &self,
        event: &Value,
        router: &CommandRouter,
        controller: &ClaudeController,
    ) -> Result<()> {
        let normalized = match self.normalize_message(event) {
            Some(n) => n,
            None => {
                // Deduplicated or malformed event
                return Ok(());
            }
        };

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

        // Route the message
        let response = router.handle(&message_text).await;

        // Send response back to the user if there is one
        if let Some(reply) = response {
            if !normalized.receive_id.is_empty() {
                let token = self.get_tenant_access_token().await?;
                self.send_text_message(&token, &normalized.receive_id_type, &normalized.receive_id, &reply).await?;
            }
        }

        // Forward Claude controller events to Feishu
        self.forward_claude_events(controller, &normalized).await?;

        Ok(())
    }

    /// Forward Claude controller events to Feishu using the same accumulator as CLI.
    async fn forward_claude_events(
        &self,
        controller: &ClaudeController,
        normalized: &NormalizedMessage,
    ) -> Result<()> {
        if normalized.receive_id.is_empty() {
            return Ok(());
        }
        let receive_id_type = &normalized.receive_id_type;
        let receive_id = &normalized.receive_id;

        let mut accumulator = EventAccumulator::new();
        let deadline = tokio::time::Instant::now() + TokioDuration::from_secs(300);
        let mut interval = tokio::time::interval(TokioDuration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut first_text_sent = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            tokio::select! {
                _ = interval.tick() => {
                    let partial = accumulator.take_output();
                    if !partial.trim().is_empty() {
                        let token = match self.get_tenant_access_token().await {
                            Ok(t) => t,
                            Err(_) => continue,
                        };
                        let _ = self.send_text_message(&token, receive_id_type, receive_id, &partial).await;
                    }
                }
                event_res = timeout(remaining, controller.recv_event()) => {
                    match event_res {
                        Ok(Some(event)) => {
                            if let ControllerEvent::PermissionRequest(req_id, tool_name) = &event {
                                let token = self.get_tenant_access_token().await?;
                                let card = self.build_permission_card(req_id, &tool_name, None);
                                self.send_interactive_card(&token, receive_id_type, receive_id, &card).await?;
                                let ctx = PendingPermissionContext {
                                    request_id: req_id.clone(),
                                    tool_name: tool_name.clone(),
                                    chat_id: receive_id.clone(),
                                    sender_open_id: normalized.sender_open_id.clone(),
                                    created_at: Instant::now(),
                                };
                                self.store_pending_permission(ctx);
                                continue;
                            }
                            let is_text = matches!(event, ControllerEvent::Text(_));
                            if accumulator.process_event(&event) {
                                break;
                            }
                            // Send first text chunk immediately for low-latency feedback
                            if is_text && !first_text_sent {
                                let partial = accumulator.take_output();
                                if !partial.trim().is_empty() {
                                    let token = match self.get_tenant_access_token().await {
                                        Ok(t) => t,
                                        Err(_) => continue,
                                    };
                                    let _ = self.send_text_message(&token, receive_id_type, receive_id, &partial).await;
                                    first_text_sent = true;
                                }
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
            let token = self.get_tenant_access_token().await?;
            self.send_text_message(&token, receive_id_type, receive_id, &reply).await?;
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // WebSocket loop
    // -----------------------------------------------------------------------

    async fn run_websocket(&self, ws_url: &str, token: &str, client_config: WsClientConfig) -> Result<()> {
        let mut current_url = ws_url.to_string();
        let mut current_token = token.to_string();
        let mut current_config = client_config;

        loop {
            let service_id = Self::extract_service_id(&current_url).unwrap_or(0);
            match self.ws_connection_loop(&current_url, &current_token, &current_config, service_id).await {
                Ok(()) => {
                    info!("Feishu WebSocket closed gracefully");
                    break;
                }
                Err(e) => {
                    warn!("WebSocket error: {}. Reconnecting in {}s...", e, current_config.reconnect_interval);
                    sleep(TokioDuration::from_secs(current_config.reconnect_interval.max(1) as u64)).await;
                    // Refresh endpoint and token before reconnect
                    match self.get_ws_endpoint().await {
                        Ok((u, cfg)) => {
                            current_url = u;
                            current_config = cfg;
                        }
                        Err(e2) => {
                            warn!("Failed to refresh WS endpoint: {}", e2);
                        }
                    }
                    match self.get_tenant_access_token().await {
                        Ok(t) => current_token = t,
                        Err(e2) => warn!("Failed to refresh token: {}", e2),
                    }
                }
            }
        }

        Ok(())
    }

    fn extract_service_id(url: &str) -> Option<i32> {
        url.split('?').nth(1)?.split('&').find(|p| p.starts_with("service_id="))?
            .strip_prefix("service_id=")?.parse().ok()
    }

    async fn ws_connection_loop(&self, ws_url: &str, token: &str, client_config: &WsClientConfig, service_id: i32) -> Result<()> {
        // Build WebSocket request (no User-Agent, matching Go SDK behavior)
        let req = ws_url.into_client_request()
            .context("Invalid WebSocket URL")?;
        let (ws_stream, response) = connect_async(req)
            .await
            .context("WebSocket connect failed")?;

        info!("Feishu WebSocket connected, response status={:?}, headers={:?}", response.status(), response.headers());

        let (write, mut read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));
        let ping_interval = Arc::new(std::sync::atomic::AtomicU64::new(client_config.ping_interval.max(1) as u64));

        // Spawn heartbeat writer
        let write_for_heartbeat = write.clone();
        let ping_interval_for_heartbeat = ping_interval.clone();
        let heartbeat_handle = {
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

        // Main read loop with timeout to detect silent disconnections
        let read_timeout_duration = TokioDuration::from_secs(
            (client_config.ping_interval.max(1) as u64) * 3
        );
        let result: Result<()> = async {
            loop {
                match timeout(read_timeout_duration, read.next()).await {
                    Ok(Some(Ok(msg))) => {
                        match msg {
                            WsMessage::Binary(data) => {
                                debug!("WS raw binary len={}", data.len());
                                // Log hex dump of first 100 bytes for debugging
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
                                    // Handle pong control frame: update ping interval from server config
                                    if frame.method == METHOD_CONTROL {
                                        if let Some(ref payload) = frame.payload {
                                            if let Ok(cfg) = serde_json::from_slice::<WsClientConfig>(payload) {
                                                debug!("Received pong with ClientConfig: ping_interval={}s", cfg.ping_interval);
                                                ping_interval.store(cfg.ping_interval.max(1) as u64, std::sync::atomic::Ordering::Relaxed);
                                            }
                                        }
                                    }
                                    if let Some(ack) = self.handle_frame(&frame, token).await? {
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
        }
        .await;

        heartbeat_handle.abort();
        result
    }

    // -----------------------------------------------------------------------
    // Frame handling
    // -----------------------------------------------------------------------

    async fn handle_frame(&self, frame: &Frame, token: &str) -> Result<Option<Frame>> {
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
                    let token = token.to_string();
                    tokio::spawn(async move {
                        if let Err(e) = platform.handle_im_payload(&payload, &token).await {
                            debug!("Not an IM event, trying card: {}", e);
                            if let Err(e2) = platform.handle_card_payload(&payload, &token).await {
                                debug!("Not a card event either: {}", e2);
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

    async fn handle_im_payload(&self, payload: &[u8], token: &str) -> Result<()> {
        let wrapper: EventWrapper =
            serde_json::from_slice(payload).context("Failed to parse IM event wrapper")?;

        debug!("IM event schema={:?} type={:?}", wrapper.schema, wrapper.header);

        let event_json = match wrapper.event {
            Some(v) => v,
            None => {
                debug!("IM event missing event body");
                return Ok(());
            }
        };

        let event: ImMessageEvent =
            serde_json::from_value(event_json).context("Failed to parse IM message event")?;

        let message = match event.message {
            Some(m) => m,
            None => {
                anyhow::bail!("IM event missing message body, event_type={:?}", wrapper.header.as_ref().and_then(|h| h.event_type.as_ref()));
            }
        };

        let chat_id = match message.chat_id {
            Some(ref id) if !id.is_empty() => id.clone(),
            _ => {
                anyhow::bail!("IM event missing chat_id, message_id={:?}", message.message_id);
            }
        };

        let sender_open_id = event
            .sender
            .as_ref()
            .and_then(|s| s.sender_id.as_ref())
            .and_then(|id| id.open_id.clone())
            .unwrap_or_default();

        // Filter by allow_from if configured
        if !self.is_allowed_sender(&sender_open_id) {
            debug!("Sender {} not in allow_from list, ignoring", sender_open_id);
            return Ok(());
        }

        let (text, media_note) = match message.message_type.as_deref() {
            Some("text") => {
                let t = if let Some(ref content_str) = message.content {
                    if let Ok(content) = serde_json::from_str::<TextMessageContent>(content_str) {
                        content.text.unwrap_or_default()
                    } else {
                        content_str.clone()
                    }
                } else {
                    String::new()
                };
                (t, None)
            }
            Some("image") => {
                let note = if let Some(ref content_str) = message.content {
                    if let Ok(v) = serde_json::from_str::<Value>(content_str) {
                        if let Some(key) = v.get("image_key").and_then(|k| k.as_str()) {
                            let msg_id = message.message_id.as_deref().unwrap_or("");
                            match self.download_message_resource(token, msg_id, key, "image").await {
                                Ok(Some((path, ctype))) => {
                                    Some(format!("[Image cached at {} ({})]", path, ctype))
                                }
                                Ok(None) => Some("[Image: download failed]".to_string()),
                                Err(e) => {
                                    warn!("Failed to download image {}: {}", key, e);
                                    Some(format!("[Image: download error {}]", e))
                                }
                            }
                        } else {
                            Some("[Image: no image_key]".to_string())
                        }
                    } else {
                        Some("[Image: invalid content]".to_string())
                    }
                } else {
                    Some("[Image: empty content]".to_string())
                };
                (String::new(), note)
            }
            Some("file") => {
                let note = if let Some(ref content_str) = message.content {
                    if let Ok(v) = serde_json::from_str::<Value>(content_str) {
                        let file_key = v.get("file_key").and_then(|k| k.as_str()).unwrap_or("");
                        let file_name = v.get("file_name").and_then(|k| k.as_str()).unwrap_or("");
                        if !file_key.is_empty() {
                            let msg_id = message.message_id.as_deref().unwrap_or("");
                            match self.download_message_resource(token, msg_id, file_key, "file").await {
                                Ok(Some((path, ctype))) => {
                                    let name = if file_name.is_empty() { &path } else { file_name };
                                    Some(format!("[File: {} cached at {} ({})]", name, path, ctype))
                                }
                                Ok(None) => Some("[File: download failed]".to_string()),
                                Err(e) => {
                                    warn!("Failed to download file {}: {}", file_key, e);
                                    Some(format!("[File: download error {}]", e))
                                }
                            }
                        } else {
                            Some("[File: no file_key]".to_string())
                        }
                    } else {
                        Some("[File: invalid content]".to_string())
                    }
                } else {
                    Some("[File: empty content]".to_string())
                };
                (String::new(), note)
            }
            Some("audio") => {
                let note = if let Some(ref content_str) = message.content {
                    if let Ok(v) = serde_json::from_str::<Value>(content_str) {
                        let file_key = v.get("file_key").and_then(|k| k.as_str()).unwrap_or("");
                        let duration = v.get("duration").and_then(|d| d.as_u64()).unwrap_or(0);
                        if !file_key.is_empty() {
                            let msg_id = message.message_id.as_deref().unwrap_or("");
                            match self.download_message_resource(token, msg_id, file_key, "file").await {
                                Ok(Some((path, ctype))) => {
                                    Some(format!(
                                        "[Audio: {}s cached at {} ({})]",
                                        duration, path, ctype
                                    ))
                                }
                                Ok(None) => Some("[Audio: download failed]".to_string()),
                                Err(e) => {
                                    warn!("Failed to download audio {}: {}", file_key, e);
                                    Some(format!("[Audio: download error {}]", e))
                                }
                            }
                        } else {
                            Some("[Audio: no file_key]".to_string())
                        }
                    } else {
                        Some("[Audio: invalid content]".to_string())
                    }
                } else {
                    Some("[Audio: empty content]".to_string())
                };
                (String::new(), note)
            }
            Some(other) => {
                debug!("Unsupported message type: {}", other);
                return Ok(());
            }
            None => (String::new(), None),
        };

        let text = if let Some(note) = media_note {
            if text.is_empty() {
                note
            } else {
                format!("{}\n{}", text, note)
            }
        } else {
            text
        };

        let trimmed = text.trim();
        if trimmed.is_empty() {
            warn!("IM event has empty text content, message_id={:?}, message_type={:?}", message.message_id, message.message_type);
            return Ok(());
        }

        // Extract mentions
        let mut mentions = Vec::new();
        if let Some(mentions_arr) = message.mentions.as_ref() {
            for m in mentions_arr {
                if let Some(oid) = m.id.as_ref().and_then(|id| id.open_id.clone()) {
                    mentions.push(MentionInfo {
                        open_id: oid,
                        name: m.name.clone(),
                        key: m.key.clone(),
                    });
                }
            }
        }

        let chat_type = message.chat_type.as_deref().unwrap_or("");
        // In group chats, only respond when the bot is @mentioned
        if chat_type == "group" {
            let bot_open_id = self.get_bot_open_id().await;
            let mentions_bot = bot_open_id.as_ref().map_or(false, |bot_id| {
                mentions.iter().any(|m| &m.open_id == bot_id)
            });
            if !mentions_bot {
                debug!("Group message does not mention bot, ignoring");
                return Ok(());
            }
        }

        let msg_id = message.message_id.clone().unwrap_or_default();

        // Deduplicate by message_id
        if !msg_id.is_empty() && self.dedup_cache.contains(&msg_id) {
            debug!("Duplicate message {} deduplicated", msg_id);
            return Ok(());
        }
        self.dedup_cache.insert(msg_id.clone());

        // Determine correct receive_id and receive_id_type based on chat type
        let (receive_id_type, receive_id) = if chat_type == "p2p" {
            ("open_id", sender_open_id.as_str())
        } else {
            ("chat_id", chat_id.as_str())
        };

        // Intercept /ll for Feishu interactive directory card
        if trimmed == "/ll" {
            info!("[Feishu] /ll command received from {}, chat_type={}, receive_id_type={}, receive_id={}, processing...",
                  sender_open_id, chat_type, receive_id_type, receive_id);
            self.on_processing_start(token, &msg_id).await;
            let result = async {
                let ctrl = self.controller.lock().await;
                let work_dir = ctrl.get_work_dir().await;
                let dir = if work_dir.is_empty() {
                    shellexpand::tilde(&self.default_dir).to_string()
                } else {
                    work_dir
                };
                drop(ctrl);
                let dirs = crate::command::builtin::list_directory_paths(&dir)
                    .unwrap_or_default();
                info!("[Feishu] /ll found {} directories under {}", dirs.len(), dir);
                if dirs.is_empty() {
                    info!("[Feishu] /ll no directories, sending text fallback");
                    self.send_text_message(token, receive_id_type, receive_id, t!("feishu.no_directories")).await?;
                } else {
                    info!("[Feishu] /ll building dir select card for {}", receive_id);
                    let card = self.build_dir_select_card(&dirs, receive_id_type, receive_id);
                    debug!("[Feishu] /ll card JSON: {}", card);
                    info!("[Feishu] /ll sending interactive card to receive_id_type={} receive_id={}", receive_id_type, receive_id);
                    self.send_interactive_card(token, receive_id_type, receive_id, &card).await?;
                    info!("[Feishu] /ll card sent successfully");
                }
                Ok::<(), anyhow::Error>(())
            }
            .await;
            if let Err(ref e) = result {
                warn!("[Feishu] /ll command failed: {}", e);
            }
            self.on_processing_complete(token, &msg_id, result.is_ok()).await;
            return result;
        }

        let normalized = NormalizedMessage {
            message_id: msg_id.clone(),
            message_type: message.message_type.clone().unwrap_or_default(),
            content: text.clone(),
            sender_open_id: sender_open_id.clone(),
            sender_name: None,
            chat_id: Some(chat_id.clone()),
            chat_type: message.chat_type.clone(),
            mentions,
            raw: json!({}),
            receive_id_type: receive_id_type.to_string(),
            receive_id: receive_id.to_string(),
        };

        self.on_processing_start(token, &msg_id).await;
        let result = self.route_message(normalized, token).await;
        self.on_processing_complete(token, &msg_id, result.is_ok()).await;
        result
    }

    async fn handle_card_payload(&self, payload: &[u8], token: &str) -> Result<()> {
        let wrapper: EventWrapper =
            serde_json::from_slice(payload).context("Failed to parse card event wrapper")?;

        let event_json = match wrapper.event {
            Some(v) => v,
            None => return Ok(()),
        };

        // Parse card action event manually since Feishu nests fields differently.
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

        // Handle directory selection card callbacks
        if let Some(ref action_value) = action_obj {
            // User-defined values are nested under action.value
            let user_value = action_value.get("value").unwrap_or(action_value);
            if let Some(cmd) = user_value.get("cmd").and_then(|v| v.as_str()) {
                if cmd == "cd" {
                    if let Some(path) = user_value.get("path").and_then(|v| v.as_str()) {
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
                            let ctrl = self.controller.lock().await;
                            ctrl.init_work_dir(path.to_string()).await;
                            drop(ctrl);
                            self.send_text_message(
                                token,
                                receive_id_type,
                                receive_id,
                                &t_fmt!("feishu.dir_changed", PATH = path),
                            )
                            .await?;
                            return Ok(());
                        }
                    }
                }
            }
        }

        // Fallback: treat other card actions as simple text commands.
        let text = format!("Card action: {:?}", action_obj);
        let open_message_id = context
            .and_then(|c| c.get("open_message_id"))
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let fallback_chat_id = context
            .and_then(|c| c.get("open_chat_id"))
            .and_then(|v| v.as_str());
        let (receive_id_type, receive_id) = if let Some(cid) = fallback_chat_id {
            ("chat_id", cid)
        } else {
            ("open_id", open_id)
        };
        let normalized = NormalizedMessage {
            message_id: open_message_id.to_string(),
            message_type: "card".to_string(),
            content: text,
            sender_open_id: open_id.to_string(),
            sender_name: None,
            chat_id: fallback_chat_id.map(|s| s.to_string()),
            chat_type: None,
            mentions: Vec::new(),
            raw: json!({}),
            receive_id_type: receive_id_type.to_string(),
            receive_id: receive_id.to_string(),
        };

        self.route_message(normalized, token).await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Routing
    // -----------------------------------------------------------------------

    async fn route_message(&self, msg: NormalizedMessage, token: &str) -> Result<()> {
        info!("Routing message from {}: {}", msg.sender_open_id, msg.content);

        let trimmed = msg.content.trim();

        // Feishu-specific command validation when no session is active
        if !trimmed.is_empty() && trimmed.starts_with('/') {
            let session_active = {
                let ctrl = self.controller.lock().await;
                ctrl.is_session_active().await
            };
            if !session_active {
                let known = ["/help", "/cd", "/cd_default", "/claude", "/ll", "/quit", "/pwd"];
                let cmd = trimmed.split_whitespace().next().unwrap_or(trimmed);
                if !known.contains(&cmd) {
                    if msg.receive_id.is_empty() {
                        return Ok(());
                    }
                    self.send_text_message(
                        token,
                        &msg.receive_id_type,
                        &msg.receive_id,
                        t!("feishu.unknown_command"),
                    )
                    .await?;
                    return Ok(());
                }
            }
        }

        // Use the CommandRouter to handle the message.
        let response = self.router.handle(&msg.content).await;

        if msg.receive_id.is_empty() {
            return Ok(());
        }

        match response {
            Some(text) => {
                // Immediate synchronous response
                self.send_text_message(token, &msg.receive_id_type, &msg.receive_id, &text).await?;
            }
            None => {
                // Message forwarded to Claude; we need to poll events and reply.
                self.poll_claude_and_reply(token, &msg.receive_id_type, &msg.receive_id).await?;
            }
        }

        Ok(())
    }

    async fn poll_claude_and_reply(
        &self,
        token: &str,
        receive_id_type: &str,
        receive_id: &str,
    ) -> Result<()> {
        let ctrl = self.controller.lock().await;
        let mut accumulator = EventAccumulator::new();
        let deadline = tokio::time::Instant::now() + TokioDuration::from_secs(300);
        let mut interval = tokio::time::interval(TokioDuration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut first_text_sent = false;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }

            tokio::select! {
                _ = interval.tick() => {
                    let partial = accumulator.take_output();
                    if !partial.trim().is_empty() {
                        let _ = self.send_text_message(token, receive_id_type, receive_id, &partial).await;
                    }
                }
                event_res = timeout(remaining, ctrl.recv_event()) => {
                    match event_res {
                        Ok(Some(event)) => {
                            if let ControllerEvent::PermissionRequest(req_id, tool_name) = &event {
                                let card = self.build_permission_card(req_id, tool_name, None);
                                let _ = self.send_interactive_card(token, receive_id_type, receive_id, &card).await;
                                let ctx = PendingPermissionContext {
                                    request_id: req_id.clone(),
                                    tool_name: tool_name.clone(),
                                    chat_id: receive_id.to_string(),
                                    sender_open_id: String::new(),
                                    created_at: Instant::now(),
                                };
                                self.store_pending_permission(ctx);
                                continue;
                            }
                            let is_text = matches!(event, ControllerEvent::Text(_));
                            if accumulator.process_event(&event) {
                                break;
                            }
                            // Send first text chunk immediately for low-latency feedback
                            if is_text && !first_text_sent {
                                let partial = accumulator.take_output();
                                if !partial.trim().is_empty() {
                                    let _ = self.send_text_message(token, receive_id_type, receive_id, &partial).await;
                                    first_text_sent = true;
                                }
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
            }
        }

        drop(ctrl);

        let reply = accumulator.take_output();
        if !reply.trim().is_empty() {
            self.send_text_message(token, receive_id_type, receive_id, &reply.trim()).await?;
        }
        Ok(())
    }


    // -----------------------------------------------------------------------
    // Chat management
    // -----------------------------------------------------------------------

    pub async fn list_chats(&self, token: &str) -> Result<Vec<ChatItem>> {
        let url = "https://open.feishu.cn/open-apis/im/v1/chats";
        let resp = self
            .http_client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
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
    // Webhook challenge & event handling
    // -----------------------------------------------------------------------

    pub fn verify_challenge(&self, body: &Value) -> Result<Value> {
        let challenge = body
            .get("challenge")
            .and_then(|v| v.as_str())
            .context("Missing challenge field")?;
        Ok(json!({ "challenge": challenge }))
    }

    pub async fn handle_webhook_event(&self, body: &Value) -> Result<Option<NormalizedMessage>> {
        if body.get("challenge").is_some() {
            anyhow::bail!("Challenge requests should be handled by verify_challenge");
        }

        let event_type = body
            .get("header")
            .and_then(|h| h.get("event_type"))
            .and_then(|v| v.as_str());

        match event_type {
            Some("im.message.receive_v1") => {
                let normalized = self.normalize_message(body);
                Ok(normalized)
            }
            Some(other) => {
                debug!("Unhandled webhook event type: {}", other);
                Ok(None)
            }
            None => {
                warn!("Webhook event missing event_type");
                Ok(None)
            }
        }
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
        token: &str,
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
            .header("Authorization", format!("Bearer {}", token))
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
        token: &str,
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
            .header("Authorization", format!("Bearer {}", token))
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

    async fn on_processing_start(&self, token: &str, message_id: &str) {
        if message_id.is_empty() {
            return;
        }
        match self.add_reaction(token, message_id, REACTION_TYPING).await {
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

    async fn on_processing_complete(&self, token: &str, message_id: &str, success: bool) {
        if message_id.is_empty() {
            return;
        }
        if let Some((_, reaction_id)) = self.pending_reactions.remove(message_id) {
            if let Err(e) = self.remove_reaction(token, message_id, &reaction_id).await {
                debug!("Failed to remove typing reaction: {}", e);
                return;
            }
            if !success {
                if let Err(e) = self.add_reaction(token, message_id, REACTION_FAILURE).await {
                    debug!("Failed to add failure reaction: {}", e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Bot identity
    // -----------------------------------------------------------------------

    async fn fetch_bot_identity(&self) -> Result<Option<BotInfo>> {
        let token = self.get_tenant_access_token().await?;
        let url = "https://open.feishu.cn/open-apis/bot/v3/info";
        let resp = self
            .http_client
            .get(url)
            .header("Authorization", format!("Bearer {}", token))
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
fn split_text_into_chunks(text: &str, max_chars: usize) -> Vec<String> {
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
                    .take_while(|(i, _)| *i < max_chars)
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

fn build_http_response(status: u16, body: &str) -> String {
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

fn build_ping_frame(service_id: i32) -> Frame {
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
fn build_ack_frame(original_frame: &Frame) -> Frame {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::controller::ClaudeController;
    use crate::command::router::CommandRouter;
    use crate::config::model::{FeishuConfig, GatewayConfig};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn test_platform() -> FeishuPlatform {
        let config = FeishuConfig {
            enabled: true,
            app_id: "${FEISHU_APP_ID}".to_string(),
            app_secret: "${FEISHU_APP_SECRET}".to_string(),
            allow_from: "*".to_string(),
            encrypt_key: "".to_string(),
            mode: "websocket".to_string(),
            webhook_bind: "0.0.0.0:3000".to_string(),
        };
        let gateway_config = GatewayConfig::default();
        let controller = Arc::new(Mutex::new(ClaudeController::new(
            gateway_config.claude.clone(),
        )));
        let default_dir = &gateway_config.default_dir;
        FeishuPlatform::new(config, default_dir, Arc::new(CommandRouter::new(controller.clone(), default_dir)), controller)
    }

    #[tokio::test]
    #[ignore = "requires network access to Feishu API"]
    async fn test_get_tenant_access_token_with_real_credentials() {
        let platform = test_platform();
        let token = platform.get_tenant_access_token().await;
        assert!(token.is_ok(), "Failed to get token: {:?}", token.err());
        let token_str = token.unwrap();
        assert!(!token_str.is_empty(), "Token should not be empty");
        println!("Got Feishu tenant_access_token: {}", token_str);
    }

    #[tokio::test]
    #[ignore = "requires network access to Feishu API"]
    async fn test_refresh_token_with_real_credentials() {
        let platform = test_platform();
        let token = platform.refresh_token().await;
        assert!(token.is_ok(), "Failed to refresh token: {:?}", token.err());
        let token_str = token.unwrap();
        assert!(!token_str.is_empty(), "Token should not be empty");
        println!("Refreshed Feishu tenant_access_token: {}", token_str);
    }

    #[tokio::test]
    async fn test_token_caching_logic() {
        let platform = test_platform();
        {
            let cached = platform.cached_token.read().await;
            assert!(cached.is_none());
        }
        let token = platform.get_tenant_access_token().await;
        if token.is_ok() {
            let cached = platform.cached_token.read().await;
            assert!(cached.is_some());
            let fetched_at = platform.token_fetched_at.read().await;
            assert!(fetched_at.is_some());
        }
    }

    #[tokio::test]
    #[ignore = "requires network access to Feishu API"]
    async fn test_list_chats_and_send_message() {
        let platform = test_platform();
        let token = platform.get_tenant_access_token().await.unwrap();

        // List chats
        let chats = platform.list_chats(&token).await;
        assert!(chats.is_ok(), "Failed to list chats: {:?}", chats.err());
        let chats = chats.unwrap();
        println!("Chats: {:?}", chats);

        // If there are chats, try sending a message to the first one
        if let Some(chat) = chats.first() {
            let result = platform.send_text_message(&token, "chat_id", &chat.chat_id, "Hello from cc-gateway test!").await;
            assert!(result.is_ok(), "Failed to send message: {:?}", result.err());
            println!("Sent message to chat: {} ({})", chat.name, chat.chat_id);
        }
    }

    #[test]
    fn test_verify_challenge() {
        let platform = test_platform();
        let body = json!({
            "challenge": "abc123",
            "token": "verification-token",
            "type": "url_verification"
        });
        let resp = platform.verify_challenge(&body).unwrap();
        assert_eq!(resp.get("challenge").unwrap().as_str().unwrap(), "abc123");
    }

    #[test]
    fn test_verify_challenge_missing_field() {
        let platform = test_platform();
        let body = json!({
            "token": "verification-token",
            "type": "url_verification"
        });
        assert!(platform.verify_challenge(&body).is_err());
    }

    #[tokio::test]
    async fn test_handle_webhook_event_text_message() {
        let platform = test_platform();
        let body = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event-123",
                "event_type": "im.message.receive_v1",
                "create_time": "1234567890"
            },
            "event": {
                "message": {
                    "message_id": "om_123",
                    "message_type": "text",
                    "content": "{\"text\":\"hello world\"}",
                    "chat_id": "oc_123",
                    "chat_type": "group"
                },
                "sender": {
                    "sender_id": {
                        "open_id": "ou_123"
                    },
                    "sender_type": "user"
                }
            }
        });

        let result = platform.handle_webhook_event(&body).await;
        assert!(result.is_ok());
        let msg = result.unwrap();
        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert_eq!(msg.message_id, "om_123");
        assert_eq!(msg.message_type, "text");
        assert_eq!(msg.sender_open_id, "ou_123");
        assert_eq!(msg.chat_id, Some("oc_123".to_string()));
    }

    #[tokio::test]
    async fn test_handle_webhook_event_challenge_refused() {
        let platform = test_platform();
        let body = json!({
            "challenge": "abc123",
            "token": "verification-token",
            "type": "url_verification"
        });
        let result = platform.handle_webhook_event(&body).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_handle_webhook_event_unhandled_type() {
        let platform = test_platform();
        let body = json!({
            "schema": "2.0",
            "header": {
                "event_id": "event-456",
                "event_type": "drive.file.created_v1"
            },
            "event": {}
        });
        let result = platform.handle_webhook_event(&body).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }
}
