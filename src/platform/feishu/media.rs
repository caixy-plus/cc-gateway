use anyhow::{Context, Result};
use reqwest_middleware::ClientWithMiddleware;
use serde_json::Value;

use crate::platform::inbound_media::{self, SavedInboundMedia};

/// Extract image keys from a Feishu post / image message content JSON.
pub fn extract_image_keys(content_str: &str) -> Vec<String> {
    let mut keys = Vec::new();
    if let Ok(v) = serde_json::from_str::<Value>(content_str) {
        if let Some(content) = v.get("content").and_then(|c| c.as_array()) {
            for line in content {
                if let Some(line_arr) = line.as_array() {
                    for segment in line_arr {
                        if let Some("img") = segment.get("tag").and_then(|t| t.as_str()) {
                            if let Some(key) = segment.get("image_key").and_then(|k| k.as_str()) {
                                keys.push(key.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    keys
}

/// Download an image from Feishu by image_key.
/// `token` is a tenant or user access token.
pub async fn download_image(image_key: &str, token: &str) -> Result<Vec<u8>> {
    let url = format!(
        "https://open.feishu.cn/open-apis/im/v1/images/{}",
        image_key
    );
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .context("Failed to download image from Feishu")?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Feishu download image failed: {} - {}", status, body);
    }

    let bytes = resp.bytes().await.context("Failed to read image bytes")?;
    Ok(bytes.to_vec())
}

/// Upload an image to Feishu and return the image_key.
/// `token` is a tenant or user access token.
pub async fn upload_image(bytes: Vec<u8>, filename: &str, token: &str) -> Result<String> {
    let url = "https://open.feishu.cn/open-apis/im/v1/images";
    let client = reqwest::Client::new();

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("image/png")
        .context("Failed to build image multipart")?;

    let form = reqwest::multipart::Form::new()
        .part("image", part)
        .text("image_type", "message");

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", token))
        .multipart(form)
        .send()
        .await
        .context("Failed to upload image to Feishu")?;

    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .context("Failed to parse upload image response")?;

    if !status.is_success() {
        anyhow::bail!("Feishu upload image failed: {} - {}", status, body);
    }

    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
    if code != 0 {
        let msg = body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        anyhow::bail!(
            "Feishu API error (upload image): code={}, msg={}",
            code,
            msg
        );
    }

    let image_key = body
        .get("data")
        .and_then(|d| d.get("image_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Upload image response missing image_key"))?;

    Ok(image_key.to_string())
}

/// Upload a file to Feishu and return the file_key.
/// `token` is a tenant or user access token.
pub async fn upload_file(bytes: Vec<u8>, filename: &str, token: &str) -> Result<String> {
    let url = "https://open.feishu.cn/open-apis/im/v1/files";
    let client = reqwest::Client::new();

    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename.to_string())
        .mime_str("application/octet-stream")
        .context("Failed to build file multipart")?;

    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("file_type", "stream")
        .text("file_name", filename.to_string());

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
        .context("Failed to parse upload file response")?;

    if !status.is_success() {
        anyhow::bail!("Feishu upload file failed: {} - {}", status, body);
    }

    let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
    if code != 0 {
        let msg = body
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        anyhow::bail!("Feishu API error (upload file): code={}, msg={}", code, msg);
    }

    let file_key = body
        .get("data")
        .and_then(|d| d.get("file_key"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Upload file response missing file_key"))?;

    Ok(file_key.to_string())
}

/// Download a message attachment via Feishu `messages/{id}/resources/{key}` API.
pub async fn download_message_resource(
    http_client: &ClientWithMiddleware,
    token: &str,
    message_id: &str,
    file_key: &str,
    resource_type: &str,
) -> Result<(Vec<u8>, String)> {
    let url = format!(
        "https://open.feishu.cn/open-apis/im/v1/messages/{}/resources/{}",
        message_id, file_key
    );
    let resp = http_client
        .get(&url)
        .query(&[("type", resource_type)])
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .with_context(|| {
            format!(
                "Failed to download Feishu {} resource {}",
                resource_type, file_key
            )
        })?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Feishu resource download failed: {} - {}", status, body);
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

    let bytes = resp
        .bytes()
        .await
        .context("Failed to read Feishu resource bytes")?
        .to_vec();
    Ok((bytes, content_type))
}

pub async fn save_downloaded_resource(
    bytes: Vec<u8>,
    content_type: &str,
) -> Result<SavedInboundMedia> {
    inbound_media::save_bytes_to_media_dir(&bytes, Some(content_type)).await
}
