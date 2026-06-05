use anyhow::Result;
use serde_json::Value;
use tracing::warn;

use super::api::extract_post_content;
use super::media::{download_message_resource, save_downloaded_resource};
use super::{FeishuPlatform, NormalizedMessage};
use crate::platform::inbound_media::{self, SavedInboundMedia};

impl FeishuPlatform {
    fn feishu_message_content_raw(msg: &NormalizedMessage) -> &str {
        msg.raw
            .get("event")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
    }

    async fn download_feishu_attachment(
        &self,
        token: &str,
        message_id: &str,
        file_key: &str,
        resource_type: &str,
    ) -> Option<SavedInboundMedia> {
        match download_message_resource(
            &self.http_client,
            token,
            message_id,
            file_key,
            resource_type,
        )
        .await
        {
            Ok((bytes, content_type)) => {
                match save_downloaded_resource(bytes, &content_type).await {
                    Ok(item) => Some(item),
                    Err(e) => {
                        warn!(
                            "Failed to save Feishu {} {}: {}",
                            resource_type, file_key, e
                        );
                        None
                    }
                }
            }
            Err(e) => {
                warn!(
                    "Failed to download Feishu {} {}: {}",
                    resource_type, file_key, e
                );
                None
            }
        }
    }

    /// Download attachments (if any) into `~/.cc-gateway/media/` and build agent markdown text.
    pub(crate) async fn resolve_inbound_content(&self, msg: &NormalizedMessage) -> Result<String> {
        let token = self.get_tenant_access_token().await?;
        let message_id = msg.message_id.as_str();
        let content_raw = Self::feishu_message_content_raw(msg);
        let mut saved: Vec<SavedInboundMedia> = Vec::new();
        let mut user_text = msg.content.clone();

        match msg.message_type.as_str() {
            "text" => {
                if let Ok(v) = serde_json::from_str::<Value>(content_raw) {
                    if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
                        user_text = text.to_string();
                    }
                    if let Some(key) = v.get("image_key").and_then(|k| k.as_str()) {
                        if let Some(item) = self
                            .download_feishu_attachment(&token, message_id, key, "image")
                            .await
                        {
                            saved.push(item);
                        }
                    } else if let Some(key) = v.get("file_key").and_then(|k| k.as_str()) {
                        if let Some(item) = self
                            .download_feishu_attachment(&token, message_id, key, "file")
                            .await
                        {
                            saved.push(item);
                        }
                    }
                }
            }
            "image" => {
                user_text.clear();
                if let Ok(v) = serde_json::from_str::<Value>(content_raw) {
                    if let Some(key) = v.get("image_key").and_then(|k| k.as_str()) {
                        if let Some(item) = self
                            .download_feishu_attachment(&token, message_id, key, "image")
                            .await
                        {
                            saved.push(item);
                        }
                    }
                }
            }
            "file" => {
                user_text.clear();
                if let Ok(v) = serde_json::from_str::<Value>(content_raw) {
                    if let Some(key) = v.get("file_key").and_then(|k| k.as_str()) {
                        if let Some(item) = self
                            .download_feishu_attachment(&token, message_id, key, "file")
                            .await
                        {
                            saved.push(item);
                        }
                    }
                }
            }
            "audio" => {
                user_text.clear();
                if let Ok(v) = serde_json::from_str::<Value>(content_raw) {
                    if let Some(key) = v.get("file_key").and_then(|k| k.as_str()) {
                        if let Some(item) = self
                            .download_feishu_attachment(&token, message_id, key, "file")
                            .await
                        {
                            saved.push(item);
                        }
                    }
                }
            }
            "post" => {
                let (text, image_keys) = extract_post_content(content_raw);
                if !text.is_empty() {
                    user_text = text;
                }
                for key in image_keys {
                    if let Some(item) = self
                        .download_feishu_attachment(&token, message_id, &key, "image")
                        .await
                    {
                        saved.push(item);
                    }
                }
            }
            "media" | "sticker" => {
                if let Ok(v) = serde_json::from_str::<Value>(content_raw) {
                    if let Some(key) = v.get("file_key").and_then(|k| k.as_str()) {
                        user_text.clear();
                        if let Some(item) = self
                            .download_feishu_attachment(&token, message_id, key, "file")
                            .await
                        {
                            saved.push(item);
                        }
                    }
                }
            }
            _ => {}
        }

        Ok(inbound_media::format_agent_message(&user_text, &saved))
    }
}
