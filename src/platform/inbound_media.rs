//! Save inbound chat attachments under `~/.cc-gateway/media/` and format paths for the agent.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::daemon::cleaner::media_dir;

#[derive(Debug, Clone)]
pub struct SavedInboundMedia {
    pub path: PathBuf,
    pub is_image: bool,
}

pub fn content_type_to_extension(content_type: &str) -> &'static str {
    let base = content_type
        .split(';')
        .next()
        .unwrap_or("application/octet-stream");
    match base {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/ogg" => "ogg",
        "audio/wav" => "wav",
        "audio/mp4" => "m4a",
        "video/mp4" => "mp4",
        "text/plain" => "txt",
        "text/markdown" => "md",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

fn resolve_extension(content_type: Option<&str>) -> &'static str {
    content_type.map(content_type_to_extension).unwrap_or("bin")
}

/// Gateway-assigned on-disk name: `{uuid}.{ext}`.
pub fn generate_storage_filename(content_type: Option<&str>) -> String {
    let ext = resolve_extension(content_type);
    let id = uuid::Uuid::new_v4();
    format!("{id}.{ext}")
}

fn is_image_content_type(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|ct| ct.starts_with("image/"))
}

/// Write bytes into `~/.cc-gateway/media/` with a unique storage filename.
pub async fn save_bytes_to_media_dir(
    bytes: &[u8],
    content_type: Option<&str>,
) -> Result<SavedInboundMedia> {
    let dir = media_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("Failed to create media dir {:?}", dir))?;

    let filename = generate_storage_filename(content_type);
    let path = dir.join(&filename);

    tokio::fs::write(&path, bytes)
        .await
        .with_context(|| format!("Failed to write media file {:?}", path))?;

    Ok(SavedInboundMedia {
        path,
        is_image: is_image_content_type(content_type),
    })
}

/// Build the user message forwarded to the agent (markdown file paths).
pub fn format_agent_message(user_text: &str, items: &[SavedInboundMedia]) -> String {
    let mut parts = Vec::new();
    let trimmed = user_text.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }

    for item in items {
        let path = item.path.to_string_lossy();
        if item.is_image {
            parts.push(format!("![]({path})"));
        } else {
            parts.push(path.to_string());
        }
    }

    if parts.is_empty() {
        String::new()
    } else {
        parts.join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_filename_is_uuid_with_extension() {
        let a = generate_storage_filename(Some("image/png"));
        let b = generate_storage_filename(Some("image/png"));
        assert_ne!(a, b);
        assert!(a.ends_with(".png"));
        let stem = a.strip_suffix(".png").unwrap();
        assert!(uuid::Uuid::parse_str(stem).is_ok());
    }

    #[test]
    fn format_agent_message_uses_empty_alt_image_markdown() {
        let storage = "550e8400-e29b-41d4-a716-446655440000.png";
        let full = format!("/home/u/.cc-gateway/media/{storage}");
        let items = vec![SavedInboundMedia {
            path: PathBuf::from(&full),
            is_image: true,
        }];
        let msg = format_agent_message("see this", &items);
        assert!(msg.contains("see this"));
        assert!(msg.contains(&format!("![]({full})")));
        assert!(!msg.contains("Media directory:"));
    }
}
