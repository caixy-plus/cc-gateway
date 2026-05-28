use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::warn;

use super::TelegramPlatform;
use crate::platform::inbound_media::{self, SavedInboundMedia};

#[derive(Clone, Copy, Debug)]
pub(crate) struct InboundContent<'a> {
    pub(crate) text: Option<&'a str>,
    pub(crate) caption: Option<&'a str>,
    pub(crate) photo: Option<&'a [TelegramPhotoSize]>,
    pub(crate) document: Option<&'a TelegramFileRef>,
    pub(crate) video: Option<&'a TelegramFileRef>,
    pub(crate) audio: Option<&'a TelegramFileRef>,
    pub(crate) voice: Option<&'a TelegramVoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TelegramFileRef {
    #[serde(rename = "file_id")]
    pub file_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TelegramPhotoSize {
    #[serde(rename = "file_id")]
    pub file_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct TelegramVoice {
    #[serde(rename = "file_id")]
    pub file_id: String,
}

impl TelegramPlatform {
    async fn download_telegram_file(&self, file_id: &str) -> Result<(Vec<u8>, String)> {
        let url = self.api_url("getFile");
        let resp = self
            .http_client
            .get(&url)
            .query(&[("file_id", file_id)])
            .send()
            .await
            .context("Telegram getFile request failed")?;
        let body: serde_json::Value = resp.json().await.context("Telegram getFile parse failed")?;
        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            anyhow::bail!("Telegram getFile error: {}", body);
        }
        let file_path = body
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(|p| p.as_str())
            .context("Telegram getFile missing file_path")?;
        let download_url = format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.config.bot_token, file_path
        );
        let file_resp = self
            .http_client
            .get(&download_url)
            .send()
            .await
            .context("Telegram file download failed")?;
        if !file_resp.status().is_success() {
            anyhow::bail!("Telegram file download HTTP {}", file_resp.status());
        }
        let content_type = file_resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .split(';')
            .next()
            .unwrap_or("application/octet-stream")
            .to_string();
        let bytes = file_resp.bytes().await?.to_vec();
        Ok((bytes, content_type))
    }

    async fn save_telegram_file(&self, file_id: &str) -> Option<SavedInboundMedia> {
        match self.download_telegram_file(file_id).await {
            Ok((bytes, content_type)) => {
                match inbound_media::save_bytes_to_media_dir(&bytes, Some(&content_type)).await {
                    Ok(item) => Some(item),
                    Err(e) => {
                        warn!("Failed to save Telegram file {}: {}", file_id, e);
                        None
                    }
                }
            }
            Err(e) => {
                warn!("Failed to download Telegram file {}: {}", file_id, e);
                None
            }
        }
    }

    pub(crate) async fn resolve_inbound_content(
        &self,
        inbound: InboundContent<'_>,
    ) -> Result<String> {
        let mut saved = Vec::new();
        let user_text = inbound
            .text
            .filter(|t| !t.is_empty())
            .or(inbound.caption.filter(|t| !t.is_empty()))
            .unwrap_or("")
            .to_string();

        if let Some(photos) = inbound.photo {
            if let Some(largest) = photos.last() {
                if let Some(item) = self.save_telegram_file(&largest.file_id).await {
                    saved.push(item);
                }
            }
        }

        if let Some(doc) = inbound.document {
            if let Some(item) = self.save_telegram_file(&doc.file_id).await {
                saved.push(item);
            }
        }

        if let Some(vid) = inbound.video {
            if let Some(item) = self.save_telegram_file(&vid.file_id).await {
                saved.push(item);
            }
        }

        if let Some(aud) = inbound.audio {
            if let Some(item) = self.save_telegram_file(&aud.file_id).await {
                saved.push(item);
            }
        }

        if let Some(v) = inbound.voice {
            if let Some(item) = self.save_telegram_file(&v.file_id).await {
                saved.push(item);
            }
        }

        Ok(inbound_media::format_agent_message(&user_text, &saved))
    }
}
