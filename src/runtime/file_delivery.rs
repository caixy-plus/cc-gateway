use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MCP_TARGET_ENV: &str = "CC_GATEWAY_MCP_TARGET";
pub const MAX_OUTBOUND_FILE_BYTES: u64 = 30 * 1024 * 1024;

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
        }
    }
}

#[async_trait::async_trait]
impl FileDelivery for FeishuFileTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        let config = crate::config::model::FeishuConfig {
            app_id: self.app_id.clone(),
            app_secret: self.app_secret.clone(),
            allow_from: "*".to_string(),
            encrypt_key: String::new(),
            enabled: true,
            mode: "websocket".to_string(),
            webhook_bind: String::new(),
            require_pairing: false,
        };
        let platform = crate::platform::feishu::FeishuPlatform::new(
            config,
            "~",
            crate::config::model::AgentProfiles::default(),
            false,
        );

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

#[async_trait::async_trait]
impl FileDelivery for TelegramFileTarget {
    async fn send_file(&self, file: OutboundFile) -> Result<SentFile> {
        let file_name = file.file_name.clone();
        let part = reqwest::multipart::Part::bytes(file.bytes)
            .file_name(file_name.clone())
            .mime_str("application/octet-stream")
            .context("Failed to build Telegram document multipart")?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", self.chat_id.clone())
            .part("document", part);

        let resp = reqwest::Client::new()
            .post(telegram_send_document_url(&self.bot_token))
            .multipart(form)
            .send()
            .await
            .context("Failed to send Telegram document")?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .context("Failed to parse Telegram sendDocument response")?;

        if !status.is_success() {
            anyhow::bail!("Telegram sendDocument failed: {} - {}", status, body);
        }
        if !body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
            let desc = body
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Telegram sendDocument API error: {}", desc);
        }

        let message_id = body
            .get("result")
            .and_then(|v| v.get("message_id"))
            .map(|v| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| v.to_string())
            })
            .ok_or_else(|| anyhow::anyhow!("Telegram sendDocument response missing message_id"))?;

        Ok(SentFile {
            platform: "telegram".to_string(),
            message_id,
            file_name,
            external_id: None,
            raw: body.get("result").cloned(),
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
