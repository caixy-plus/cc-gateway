//! Save inbound chat attachments under `~/.cc-gateway/media/` and format paths for the agent.

use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::daemon::cleaner::media_dir;

#[derive(Debug, Clone)]
pub struct SavedInboundMedia {
    pub path: PathBuf,
    pub is_image: bool,
}

fn ext_is_image(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg")
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

fn extension_from_filename(filename: &str) -> Option<String> {
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim()
        .trim_start_matches('.')
        .to_ascii_lowercase();
    if ext.is_empty() {
        return None;
    }
    // Keep it safe and predictable for on-disk filenames.
    if !ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if ext.len() > 10 {
        return None;
    }
    Some(ext)
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

fn is_image_from_metadata(filename: Option<&str>, content_type: Option<&str>) -> bool {
    if is_image_content_type(content_type) {
        return true;
    }
    filename
        .and_then(extension_from_filename)
        .is_some_and(|ext| ext_is_image(&ext))
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

/// Like [`save_bytes_to_media_dir`], but try to preserve a useful extension from an upstream filename.
///
/// This is important for platforms where `Content-Type` is often `application/octet-stream`
/// (e.g. Telegram file download), but the upstream `file_path` contains a real extension.
pub async fn save_bytes_to_media_dir_with_upstream_name(
    bytes: &[u8],
    upstream_name: &str,
    content_type: Option<&str>,
) -> Result<SavedInboundMedia> {
    let dir = media_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("Failed to create media dir {:?}", dir))?;

    let ext = extension_from_filename(upstream_name)
        .unwrap_or_else(|| resolve_extension(content_type).to_string());
    let id = uuid::Uuid::new_v4();
    let filename = format!("{id}.{ext}");
    let path = dir.join(&filename);

    tokio::fs::write(&path, bytes)
        .await
        .with_context(|| format!("Failed to write media file {:?}", path))?;

    Ok(SavedInboundMedia {
        path,
        is_image: is_image_from_metadata(Some(upstream_name), content_type),
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
    fn filename_extension_parsing_is_sanitized() {
        assert_eq!(extension_from_filename("foo.PDF").as_deref(), Some("pdf"));
        assert_eq!(extension_from_filename("noext"), None);
        assert_eq!(extension_from_filename("bad.ext!"), None);
        assert_eq!(extension_from_filename("x.veryveryverylongext"), None);
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
