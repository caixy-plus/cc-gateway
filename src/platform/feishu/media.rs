use anyhow::{Context, Result};

use super::FeishuPlatform;

impl FeishuPlatform {
    // -----------------------------------------------------------------------
    // Media download
    // -----------------------------------------------------------------------

    /// Download a message resource (image/file/audio) from Feishu and cache it locally.
    pub(crate) async fn download_message_resource(
        &self,
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

        let cache_dir = crate::daemon::cleaner::media_dir();
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
        tracing::info!(
            "[Feishu] Cached {} resource at {} ({} bytes, {})",
            resource_type,
            path_str,
            bytes.len(),
            content_type
        );
        Ok(Some((path_str, content_type)))
    }
}
