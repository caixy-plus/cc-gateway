use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MCP_TARGET_ENV: &str = "CC_GATEWAY_MCP_TARGET";
pub const MAX_OUTBOUND_FILE_BYTES: u64 = 30 * 1024 * 1024;
/// Feishu image upload API limit (see open.feishu.cn im/v1/images).
pub const FEISHU_MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpContext {
    pub delivery: McpDeliveryTarget,
}

impl McpContext {
    pub fn to_env_json(&self) -> Result<String> {
        serde_json::to_string(self).context("Failed to serialize MCP delivery target")
    }

    pub fn from_env_json(raw: &str) -> Result<Self> {
        serde_json::from_str(raw).context("Failed to parse MCP delivery target")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "platform", rename_all = "snake_case")]
pub enum McpDeliveryTarget {
    Feishu(FeishuFileTarget),
    Telegram(TelegramFileTarget),
    Qq(QqFileTarget),
    WebUi(crate::web::files::WebUiFileTarget),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeishuFileTarget {
    pub app_id: String,
    pub app_secret: String,
    pub chat_id: String,
    pub receive_id_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramFileTarget {
    pub bot_token: String,
    pub chat_id: String,
    pub proxy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QqFileTarget {
    pub app_id: String,
    pub app_secret: String,
    pub sandbox: bool,
    pub chat: crate::platform::qq::QqFileChatTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundFile {
    pub path: PathBuf,
    pub file_name: String,
    pub file_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SentFile {
    pub platform: String,
    pub message_id: String,
    pub file_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[async_trait::async_trait]
pub trait FileDelivery {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile>;
}

#[async_trait::async_trait]
impl FileDelivery for McpDeliveryTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        match self {
            McpDeliveryTarget::Feishu(target) => target.send_file(file).await,
            McpDeliveryTarget::Telegram(target) => target.send_file(file).await,
            McpDeliveryTarget::Qq(target) => target.send_file(file).await,
            McpDeliveryTarget::WebUi(target) => target.send_file(file).await,
        }
    }
}

#[async_trait::async_trait]
impl FileDelivery for crate::web::files::WebUiFileTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        let content_type = mime_guess_from_name(&file.file_name);
        let saved = crate::platform::inbound_media::save_bytes_to_media_dir_with_upstream_name(
            &file.bytes,
            &file.file_name,
            Some(content_type),
        )
        .await?;
        let media_key = crate::web::files::media_storage_basename(&saved.path)?;
        let size = file.bytes.len() as u64;
        crate::web::files::broadcast_file_attachment(
            &self.session_id,
            "assistant",
            &media_key,
            &file.file_name,
            size,
            saved.is_image,
        );
        Ok(SentFile {
            platform: "webui".to_string(),
            message_id: media_key,
            file_name: file.file_name,
            external_id: None,
            raw: None,
        })
    }
}

/// Whether MCP outbound should use platform **image** APIs (inline preview), not generic file/document.
pub fn outbound_file_is_image(file: &OutboundFile) -> bool {
    if file.file_type == "image" {
        return true;
    }
    image_extension_from_name(&file.file_name)
}

fn image_extension_from_name(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ico" | "tiff" | "tif"
            | "heic" | "heif"
    )
}

fn mime_guess_from_name(name: &str) -> &'static str {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain",
        _ => "application/octet-stream",
    }
}

#[async_trait::async_trait]
impl FileDelivery for FeishuFileTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        let config = crate::config::model::FeishuConfig {
            app_id: self.app_id.clone(),
            app_secret: self.app_secret.clone(),
            enabled: true,
            require_pairing: false,
        };
        let platform = crate::platform::feishu::FeishuPlatform::new(
            config,
            "~",
            crate::config::model::AgentProfiles::default(),
            false,
        );

        if outbound_file_is_image(&file) {
            if file.bytes.len() as u64 > FEISHU_MAX_IMAGE_BYTES {
                anyhow::bail!(
                    "{}",
                    crate::t_fmt!(
                        "feishu.image_too_large",
                        MB = FEISHU_MAX_IMAGE_BYTES / 1024 / 1024
                    )
                );
            }
            let token = platform
                .token_manager
                .get_tenant_access_token()
                .await
                .context("Feishu tenant token for image upload")?;
            let mime = mime_guess_from_name(&file.file_name);
            let image_key = crate::platform::feishu::media::upload_image(
                file.bytes,
                &file.file_name,
                mime,
                &token,
            )
            .await?;
            let message_id = platform
                .send_image_message(&self.receive_id_type, &self.chat_id, &image_key)
                .await?;
            return Ok(SentFile {
                platform: "feishu".to_string(),
                message_id,
                file_name: file.file_name,
                external_id: Some(image_key),
                raw: None,
            });
        }

        let file_key = platform
            .upload_file(&file.file_type, &file.file_name, file.bytes)
            .await?;
        let message_id = platform
            .send_file_message(&self.receive_id_type, &self.chat_id, &file_key)
            .await?;

        Ok(SentFile {
            platform: "feishu".to_string(),
            message_id,
            file_name: file.file_name,
            external_id: Some(file_key),
            raw: None,
        })
    }
}

pub(crate) fn telegram_send_document_url(bot_token: &str) -> String {
    format!("https://api.telegram.org/bot{}/sendDocument", bot_token)
}

pub(crate) fn telegram_send_photo_url(bot_token: &str) -> String {
    format!("https://api.telegram.org/bot{}/sendPhoto", bot_token)
}

#[async_trait::async_trait]
impl FileDelivery for TelegramFileTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        let file_name = file.file_name.clone();
        let mime = mime_guess_from_name(&file_name);
        let (url, field) = if outbound_file_is_image(&file) {
            (telegram_send_photo_url(&self.bot_token), "photo")
        } else {
            (telegram_send_document_url(&self.bot_token), "document")
        };

        let part = reqwest::multipart::Part::bytes(file.bytes)
            .file_name(file_name.clone())
            .mime_str(mime)
            .context("Failed to build Telegram multipart")?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", self.chat_id.clone())
            .part(field, part);

        let client = crate::platform::telegram::build_http_client(&self.proxy);
        let resp = client
            .post(url)
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("Failed to send Telegram {field}"))?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .with_context(|| format!("Failed to parse Telegram {field} response"))?;

        if !status.is_success() {
            anyhow::bail!("Telegram {field} failed: {} - {}", status, body);
        }
        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            let desc = body
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Telegram {field} API error: {}", desc);
        }

        let message_id = body
            .get("result")
            .and_then(|v| v.get("message_id"))
            .map(|v| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| v.to_string())
            })
            .ok_or_else(|| {
                anyhow::anyhow!("Telegram {field} response missing message_id")
            })?;

        Ok(SentFile {
            platform: "telegram".to_string(),
            message_id,
            file_name,
            external_id: None,
            raw: body.get("result").cloned(),
        })
    }
}

#[async_trait::async_trait]
impl FileDelivery for QqFileTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        let client = crate::platform::qq::QqApiClient::new(
            self.app_id.clone(),
            self.app_secret.clone(),
            self.sandbox,
        );
        let message_id = client
            .send_rich_media_file(&self.chat, &file.file_name, &file.file_type, &file.bytes)
            .await
            .with_context(|| format!("QQ send_file failed for {}", file.path.display()))?;
        Ok(SentFile {
            platform: "qq".to_string(),
            message_id,
            file_name: file.file_name,
            external_id: None,
            raw: None,
        })
    }
}

/// Known file extensions and their Feishu-compatible file_type mapping.
pub fn detect_file_type(path: &str) -> &str {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "pdf" => "pdf",
        "doc" => "doc",
        "docx" => "docx",
        "xls" => "xls",
        "xlsx" => "xlsx",
        "ppt" => "ppt",
        "pptx" => "pptx",
        "csv" => "csv",
        "txt" | "md" | "rs" | "py" | "js" | "ts" | "json" | "toml" | "yaml" | "yml" | "log"
        | "html" | "css" | "sh" | "bash" | "sql" => "stream",
        "mp4" | "mov" | "avi" => "mp4",
        "opus" | "ogg" => "opus",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => "image",
        _ => "stream",
    }
}

pub async fn validate_outbound_file(
    path_str: &str,
    file_name: Option<&str>,
) -> Result<OutboundFile> {
    let expanded = shellexpand::tilde(path_str).to_string();
    let path = Path::new(&expanded);

    if !path.exists() {
        anyhow::bail!("文件不存在: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!("路径不是文件: {}", path.display());
    }

    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_OUTBOUND_FILE_BYTES {
        anyhow::bail!("文件太大: {} bytes (最大 30MB)", metadata.len());
    }
    if metadata.len() == 0 {
        anyhow::bail!("不允许发送空文件");
    }

    let resolved_file_name = file_name.map(str::to_string).unwrap_or_else(|| {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string()
    });
    let bytes = tokio::fs::read(path).await?;

    Ok(OutboundFile {
        path: path.to_path_buf(),
        file_name: resolved_file_name,
        file_type: detect_file_type(path_str).to_string(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file(name: &str, file_type: &str) -> OutboundFile {
        OutboundFile {
            path: PathBuf::from(name),
            file_name: name.to_string(),
            file_type: file_type.to_string(),
            bytes: vec![1, 2, 3],
        }
    }

    #[test]
    fn outbound_file_is_image_by_type_or_extension() {
        assert!(outbound_file_is_image(&sample_file("x.bin", "image")));
        assert!(outbound_file_is_image(&sample_file("photo.png", "stream")));
        assert!(!outbound_file_is_image(&sample_file("doc.pdf", "pdf")));
    }

    #[test]
    fn telegram_photo_url_differs_from_document() {
        let token = "123:ABC";
        assert!(telegram_send_photo_url(token).contains("sendPhoto"));
        assert!(telegram_send_document_url(token).contains("sendDocument"));
    }
}
