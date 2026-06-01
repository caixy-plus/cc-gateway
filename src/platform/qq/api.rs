//! QQ 开放平台 Bot API v2 — access token, gateway, send message.

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
const TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const API_BASE: &str = "https://api.sgroup.qq.com";
const API_BASE_SANDBOX: &str = "https://sandbox.api.sgroup.qq.com";

/// `GROUP_AND_C2C_EVENT` — C2C + group @ messages (see QQ intents docs).
pub const INTENTS_GROUP_AND_C2C: u64 = 1 << 25;

#[derive(Clone)]
pub struct QqApiClient {
    app_id: String,
    app_secret: String,
    pub sandbox: bool,
    http: reqwest::Client,
    token: Arc<RwLock<Option<CachedToken>>>,
}

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct GatewayBotResponse {
    pub url: String,
    #[serde(default)]
    pub shards: u32,
}

impl QqApiClient {
    pub fn new(app_id: String, app_secret: String, sandbox: bool) -> Self {
        Self {
            app_id,
            app_secret,
            sandbox,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(45))
                .build()
                .expect("failed to build QQ HTTP client"),
            token: Arc::new(RwLock::new(None)),
        }
    }

    fn api_base(&self) -> &'static str {
        if self.sandbox {
            API_BASE_SANDBOX
        } else {
            API_BASE
        }
    }

    pub async fn access_token(&self) -> Result<String> {
        {
            let guard = self.token.read().await;
            if let Some(ref cached) = *guard {
                if Instant::now() < cached.expires_at {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        let body = json!({
            "appId": self.app_id,
            "clientSecret": self.app_secret,
        });
        let resp = self
            .http
            .post(TOKEN_URL)
            .json(&body)
            .send()
            .await
            .context("QQ getAppAccessToken request failed")?;
        let status = resp.status();
        let parsed: TokenResponse = resp
            .json()
            .await
            .with_context(|| format!("QQ getAppAccessToken invalid JSON (HTTP {})", status))?;

        let expires_in = match &parsed.expires_in {
            Value::Number(n) => n.as_u64().unwrap_or(7200),
            Value::String(s) => s.parse().unwrap_or(7200),
            _ => 7200,
        };
        let ttl = expires_in.saturating_sub(120).max(60);
        let access_token = parsed.access_token;
        {
            let mut guard = self.token.write().await;
            *guard = Some(CachedToken {
                access_token: access_token.clone(),
                expires_at: Instant::now() + Duration::from_secs(ttl),
            });
        }
        Ok(access_token)
    }

    fn auth_header(&self, token: &str) -> String {
        format!("QQBot {}", token)
    }

    pub async fn fetch_gateway(&self) -> Result<GatewayBotResponse> {
        let token = self.access_token().await?;
        let url = format!("{}/gateway/bot", self.api_base());
        let resp = self
            .http
            .get(&url)
            .header("Authorization", self.auth_header(&token))
            .send()
            .await
            .context("QQ gateway/bot request failed")?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("QQ gateway/bot failed (HTTP {}): {}", status, body);
        }
        serde_json::from_str(&body)
            .with_context(|| format!("QQ gateway/bot invalid JSON: {}", body))
    }

    pub async fn send_c2c_text(&self, openid: &str, text: &str) -> Result<()> {
        let token = self.access_token().await?;
        let url = format!("{}/v2/users/{}/messages", self.api_base(), openid);
        self.post_text_message(&url, &token, text).await
    }

    pub async fn send_group_text(&self, group_openid: &str, text: &str) -> Result<()> {
        let token = self.access_token().await?;
        let url = format!("{}/v2/groups/{}/messages", self.api_base(), group_openid);
        self.post_text_message(&url, &token, text).await
    }

    /// Upload local bytes and send as rich media (`msg_type` 7).
    pub async fn send_rich_media_file(
        &self,
        target: &QqFileChatTarget,
        file_name: &str,
        gateway_file_type: &str,
        bytes: &[u8],
    ) -> Result<String> {
        let (qq_file_type, c2c_only) = classify_qq_media_type(file_name, gateway_file_type);
        if matches!(target, QqFileChatTarget::Group { .. }) && c2c_only {
            anyhow::bail!("{}", crate::t!("qq.send_file_group_unsupported"));
        }

        let token = self.access_token().await?;
        let upload_url = target.files_upload_url(self.api_base());
        let file_data = base64::engine::general_purpose::STANDARD.encode(bytes);
        let upload_body = json!({
            "file_type": qq_file_type,
            "url": "",
            "srv_send_msg": false,
            "file_data": file_data,
        });
        let upload_resp = self
            .http
            .post(&upload_url)
            .header("Authorization", self.auth_header(&token))
            .json(&upload_body)
            .send()
            .await
            .with_context(|| format!("QQ upload media failed for {}", upload_url))?;
        let upload_status = upload_resp.status();
        let upload_json: Value = upload_resp
            .json()
            .await
            .with_context(|| format!("QQ upload media invalid JSON (HTTP {})", upload_status))?;
        if !upload_status.is_success() {
            anyhow::bail!("QQ upload media HTTP {}: {}", upload_status, upload_json);
        }

        let file_info = upload_json.get("file_info").cloned().ok_or_else(|| {
            anyhow::anyhow!("QQ upload response missing file_info: {}", upload_json)
        })?;

        let caption = crate::t_fmt!("qq.sent_file_caption", NAME = file_name);
        let messages_url = target.messages_url(self.api_base());
        let send_body = json!({
            "msg_type": 7,
            "content": caption,
            "media": file_info,
        });
        let send_resp = self
            .http
            .post(&messages_url)
            .header("Authorization", self.auth_header(&token))
            .json(&send_body)
            .send()
            .await
            .with_context(|| format!("QQ send rich media failed for {}", messages_url))?;
        let send_status = send_resp.status();
        let send_json: Value = send_resp
            .json()
            .await
            .with_context(|| format!("QQ send rich media invalid JSON (HTTP {})", send_status))?;
        if !send_status.is_success() {
            anyhow::bail!("QQ send rich media HTTP {}: {}", send_status, send_json);
        }

        let message_id = send_json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        Ok(message_id)
    }

    async fn post_text_message(&self, url: &str, token: &str, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        for chunk in split_text_chunks(text, 3500) {
            let payload = json!({
                "msg_type": 0,
                "content": chunk,
            });
            let resp = self
                .http
                .post(url)
                .header("Authorization", self.auth_header(token))
                .json(&payload)
                .send()
                .await
                .with_context(|| format!("QQ send message failed for {}", url))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("QQ send message HTTP {}: {}", status, body);
            }
        }
        Ok(())
    }
}

/// Serializable chat target for MCP `send_file` (`McpDeliveryTarget::Qq`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QqFileChatTarget {
    C2c { openid: String },
    Group { group_openid: String },
}

impl QqFileChatTarget {
    pub fn from_channel_id(channel_id: &str) -> Option<Self> {
        if let Some(openid) = channel_id.strip_prefix("u:") {
            return Some(Self::C2c {
                openid: openid.to_string(),
            });
        }
        if let Some(gid) = channel_id.strip_prefix("g:") {
            return Some(Self::Group {
                group_openid: gid.to_string(),
            });
        }
        None
    }

    fn files_upload_url(&self, api_base: &str) -> String {
        match self {
            Self::C2c { openid } => format!("{}/v2/users/{}/files", api_base, openid),
            Self::Group { group_openid } => {
                format!("{}/v2/groups/{}/files", api_base, group_openid)
            }
        }
    }

    fn messages_url(&self, api_base: &str) -> String {
        match self {
            Self::C2c { openid } => format!("{}/v2/users/{}/messages", api_base, openid),
            Self::Group { group_openid } => {
                format!("{}/v2/groups/{}/messages", api_base, group_openid)
            }
        }
    }
}

/// Map gateway `detect_file_type` + filename to QQ `file_type` (1 image, 2 video, 3 voice, 4 file).
/// Second return value: `true` when only C2C supports this type (e.g. generic files in groups).
pub fn classify_qq_media_type(file_name: &str, gateway_file_type: &str) -> (u32, bool) {
    let ext = std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => return (1, false),
        "mp4" | "mov" | "avi" => return (2, false),
        "silk" | "wav" | "mp3" | "flac" | "ogg" | "opus" => return (3, false),
        _ => {}
    }
    match gateway_file_type {
        "image" => (1, false),
        "mp4" => (2, false),
        "opus" => (3, false),
        _ => (4, true),
    }
}

/// Extract user-visible text from a dispatch event `d` object.
pub fn extract_message_text(d: &Value) -> Option<String> {
    d.get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty())
}

pub fn split_text_chunks(text: &str, max_chars: usize) -> Vec<String> {
    if text.chars().count() <= max_chars {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if current.chars().count() >= max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_content_from_event_payload() {
        let d = json!({ "content": "hello qq" });
        assert_eq!(extract_message_text(&d).as_deref(), Some("hello qq"));
    }

    #[test]
    fn splits_long_text() {
        let text = "a".repeat(4000);
        let chunks = split_text_chunks(&text, 3500);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn classify_media_types() {
        assert_eq!(classify_qq_media_type("a.png", "image"), (1, false));
        assert_eq!(classify_qq_media_type("clip.mp4", "mp4"), (2, false));
        assert_eq!(classify_qq_media_type("note.pdf", "pdf"), (4, true));
        assert_eq!(classify_qq_media_type("readme.md", "stream"), (4, true));
    }

    #[test]
    fn file_chat_target_from_channel_id() {
        let c2c = QqFileChatTarget::from_channel_id("u:openid123").unwrap();
        assert!(matches!(c2c, QqFileChatTarget::C2c { .. }));
        let grp = QqFileChatTarget::from_channel_id("g:grp456").unwrap();
        assert!(matches!(grp, QqFileChatTarget::Group { .. }));
    }
}
