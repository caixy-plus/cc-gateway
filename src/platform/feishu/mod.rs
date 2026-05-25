#![allow(dead_code)]
pub(crate) mod auth_middleware;
pub(crate) mod cards;
pub(crate) mod handle;
pub(crate) mod interaction;
pub(crate) mod media;
pub(crate) mod webhook;
pub(crate) mod ws;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::DashMap;
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::{Mutex, RwLock};
use tracing::warn;

use crate::config::model::{AgentSettings, FeishuConfig};
use crate::session::channel_manager::ActiveClaudeRuntime;

// Re-export commonly used items from child modules
pub(crate) use handle::{
    build_ack_frame, build_http_response, build_ping_frame, extract_post_content,
};

// ---------------------------------------------------------------------------
// Constants for Feishu pbbp2 WebSocket protocol
// ---------------------------------------------------------------------------

pub(crate) const METHOD_CONTROL: i32 = 0;
pub(crate) const METHOD_DATA: i32 = 1;
pub(crate) const SERVICE_SYSTEM: i32 = 0;
pub(crate) const SERVICE_IM: i32 = 1;
pub(crate) const SERVICE_CARD: i32 = 2;
pub(crate) const HEARTBEAT_INTERVAL_SECS: u64 = 30;
pub(crate) const FEISHU_MAX_TEXT_CHARS: usize = 3500;
pub(crate) const FEISHU_CHUNK_DELAY_MS: u64 = 300;
pub(crate) const REACTION_TYPING: &str = "Typing";
pub(crate) const REACTION_FAILURE: &str = "CrossMark";

// ---------------------------------------------------------------------------
// Per-channel runtime
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub(crate) struct FeishuChannelRuntime {
    pub(crate) channel_session: crate::session::channel_model::ChannelSession,
    pub(crate) active_claude: Option<ActiveClaudeRuntime>,
    pub(crate) receive_id_type: String,
    pub(crate) receive_id: String,
    pub(crate) poll_lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FeishuShutdownNoticeTarget {
    pub(crate) receive_id_type: String,
    pub(crate) receive_id: String,
}

impl FeishuChannelRuntime {
    pub(crate) fn new(
        channel_session: crate::session::channel_model::ChannelSession,
        receive_id_type: String,
        receive_id: String,
    ) -> Self {
        Self {
            channel_session,
            active_claude: None,
            receive_id_type,
            receive_id,
            poll_lock: Arc::new(Mutex::new(())),
        }
    }

    pub(crate) fn shutdown_notice_target(&self) -> Option<FeishuShutdownNoticeTarget> {
        self.active_claude.as_ref()?;
        Some(FeishuShutdownNoticeTarget {
            receive_id_type: self.receive_id_type.clone(),
            receive_id: self.receive_id.clone(),
        })
    }

    pub(crate) fn set_work_dir(&mut self, work_dir: String) {
        self.channel_session.work_dir = work_dir;
    }
}

// ---------------------------------------------------------------------------
// API response structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct TenantAccessTokenResp {
    pub(crate) code: i32,
    #[serde(default)]
    pub(crate) msg: Option<String>,
    #[serde(rename = "tenant_access_token")]
    pub(crate) tenant_access_token: String,
}

#[derive(Debug, Deserialize)]
pub struct ChatItem {
    pub(crate) chat_id: String,
    pub(crate) name: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsEndpointResp {
    #[serde(default)]
    pub(crate) code: i32,
    #[serde(default)]
    pub(crate) msg: String,
    #[serde(default)]
    pub(crate) data: Option<WsEndpointData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct WsEndpointData {
    #[serde(rename = "URL")]
    pub(crate) url: Option<String>,
    #[serde(rename = "ClientConfig")]
    pub(crate) client_config: Option<WsClientConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct WsClientConfig {
    #[serde(rename = "ReconnectCount")]
    pub(crate) reconnect_count: i32,
    #[serde(rename = "ReconnectInterval")]
    pub(crate) reconnect_interval: i32,
    #[serde(rename = "ReconnectNonce")]
    pub(crate) reconnect_nonce: i32,
    #[serde(rename = "PingInterval")]
    pub(crate) ping_interval: i32,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReactionCreateResp {
    pub(crate) code: i32,
    #[serde(default)]
    pub(crate) msg: Option<String>,
    #[serde(default)]
    pub(crate) data: Option<ReactionData>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReactionData {
    #[serde(rename = "reaction_id")]
    pub(crate) reaction_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct BotInfo {
    #[serde(rename = "open_id")]
    pub(crate) open_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct BotInfoResp {
    pub(crate) code: i32,
    #[serde(default)]
    pub(crate) msg: Option<String>,
    #[serde(default)]
    pub(crate) bot: Option<BotInfo>,
}

// ---------------------------------------------------------------------------
// Event payload structs (serde)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EventWrapper {
    #[serde(default)]
    pub(crate) schema: String,
    #[serde(default)]
    pub(crate) header: Option<EventHeader>,
    #[serde(default)]
    pub(crate) event: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct EventHeader {
    #[serde(rename = "event_id")]
    pub(crate) event_id: Option<String>,
    #[serde(rename = "event_type")]
    pub(crate) event_type: Option<String>,
    #[serde(rename = "create_time")]
    pub(crate) create_time: Option<String>,
    #[serde(rename = "token")]
    pub(crate) token: Option<String>,
    #[serde(rename = "app_id")]
    pub(crate) app_id: Option<String>,
    #[serde(rename = "tenant_key")]
    pub(crate) tenant_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ImMessageEvent {
    #[serde(default)]
    pub(crate) sender: Option<SenderInfo>,
    #[serde(default)]
    pub(crate) message: Option<MessageInfo>,
    #[serde(default)]
    pub(crate) mentions: Option<Vec<MentionEventInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SenderInfo {
    #[serde(rename = "sender_id")]
    pub(crate) sender_id: Option<OpenIdInfo>,
    #[serde(rename = "sender_type")]
    pub(crate) sender_type: Option<String>,
    #[serde(rename = "tenant_key")]
    pub(crate) tenant_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct OpenIdInfo {
    #[serde(rename = "open_id")]
    pub(crate) open_id: Option<String>,
    #[serde(rename = "union_id")]
    pub(crate) union_id: Option<String>,
    #[serde(rename = "user_id")]
    pub(crate) user_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MessageInfo {
    #[serde(rename = "message_id")]
    pub(crate) message_id: Option<String>,
    #[serde(rename = "root_id")]
    pub(crate) root_id: Option<String>,
    #[serde(rename = "parent_id")]
    pub(crate) parent_id: Option<String>,
    #[serde(rename = "create_time")]
    pub(crate) create_time: Option<String>,
    #[serde(rename = "chat_id")]
    pub(crate) chat_id: Option<String>,
    #[serde(rename = "chat_type")]
    pub(crate) chat_type: Option<String>,
    #[serde(rename = "message_type")]
    pub(crate) message_type: Option<String>,
    #[serde(rename = "content")]
    pub(crate) content: Option<String>,
    #[serde(default)]
    pub(crate) mentions: Option<Vec<MentionEventInfo>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MentionEventInfo {
    #[serde(rename = "key")]
    pub(crate) key: Option<String>,
    #[serde(rename = "id")]
    pub(crate) id: Option<OpenIdInfo>,
    #[serde(rename = "name")]
    pub(crate) name: Option<String>,
    #[serde(rename = "tenant_key")]
    pub(crate) tenant_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TextMessageContent {
    pub(crate) text: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CardActionEvent {
    #[serde(rename = "open_message_id")]
    pub(crate) open_message_id: Option<String>,
    #[serde(rename = "open_id")]
    pub(crate) open_id: Option<String>,
    #[serde(rename = "tenant_key")]
    pub(crate) tenant_key: Option<String>,
    #[serde(default)]
    pub(crate) action: Option<Value>,
    #[serde(default)]
    #[serde(rename = "trigger_time")]
    pub(crate) trigger_time: Option<String>,
}

// ---------------------------------------------------------------------------
// Platform / runtime types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct PendingPermissionContext {
    pub request_id: String,
    pub tool_name: String,
    pub chat_id: String,
    pub sender_open_id: String,
    pub created_at: Instant,
}

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
    pub receive_id_type: String,
    pub receive_id: String,
}

#[derive(Clone, Debug)]
pub struct MentionInfo {
    pub open_id: String,
    pub name: Option<String>,
    pub key: Option<String>,
}

// ---------------------------------------------------------------------------
// Utility types
// ---------------------------------------------------------------------------

pub(crate) struct DedupCache {
    inner: DashMap<String, Instant>,
    ttl: Duration,
}

impl DedupCache {
    pub(crate) fn new(ttl_secs: u64) -> Self {
        Self {
            inner: DashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub(crate) fn insert(&self, key: String) {
        self.inner.insert(key, Instant::now());
        self.cleanup();
    }

    pub(crate) fn contains(&self, key: &str) -> bool {
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

pub(crate) struct RateLimiter {
    inner: DashMap<String, Vec<Instant>>,
    max_requests: usize,
    window: Duration,
}

impl RateLimiter {
    pub(crate) fn new(max_requests: usize, window_secs: u64) -> Self {
        Self {
            inner: DashMap::new(),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub(crate) fn check(&self, key: &str) -> bool {
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

pub(crate) struct AnomalyTracker {
    inner: DashMap<String, (u32, Instant)>,
    threshold: u32,
    ttl: Duration,
}

impl AnomalyTracker {
    pub(crate) fn new(threshold: u32, ttl_secs: u64) -> Self {
        Self {
            inner: DashMap::new(),
            threshold,
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    pub(crate) fn record(&self, key: &str, status: u16) {
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
                        new_count, key,
                        now.duration_since(first_seen).as_secs()
                    );
                }
            }
        } else {
            self.inner.insert(key.to_string(), (1, now));
        }
    }
}

// ---------------------------------------------------------------------------
// FeishuPlatform struct
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FeishuPlatform {
    pub(crate) config: FeishuConfig,
    pub(crate) default_dir: String,
    pub(crate) claude_config: AgentSettings,
    pub(crate) show_thinking: Arc<AtomicBool>,
    pub(crate) http_client: ClientWithMiddleware,
    pub(crate) dedup_cache: Arc<DedupCache>,
    pub(crate) pending_permissions: Arc<DashMap<String, PendingPermissionContext>>,
    pub(crate) interaction_store: Arc<interaction::InteractionStore>,
    pub(crate) token_manager: auth_middleware::TokenManager,
    pub(crate) pending_reactions: Arc<DashMap<String, String>>,
    pub(crate) bot_identity: Arc<RwLock<Option<BotInfo>>>,
    pub(crate) rate_limiter: Arc<RateLimiter>,
    pub(crate) anomaly_tracker: Arc<AnomalyTracker>,
    pub(crate) channels: Arc<DashMap<String, FeishuChannelRuntime>>,
}

// ---------------------------------------------------------------------------
// Platform trait implementation
// ---------------------------------------------------------------------------

use crate::platform::Platform;
use async_trait::async_trait;

#[async_trait]
impl Platform for FeishuPlatform {
    async fn run(&self) -> anyhow::Result<()> {
        match self.config.mode.as_str() {
            "webhook" => self.run_webhook().await,
            _ => self.run().await,
        }
    }

    async fn shutdown(&self) {
        self.shutdown_all_sessions().await;
    }
}
