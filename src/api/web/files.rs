//! WebUI chat file attachments (inbound upload + MCP `send_file` outbound).

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use crate::daemon::cleaner::media_dir;
use crate::platform::inbound_media::{save_bytes_to_media_dir_with_upstream_name, SavedInboundMedia};
use crate::runtime::file_delivery::{
    FileDelivery, McpContext, McpDeliveryTarget, MAX_OUTBOUND_FILE_BYTES,
};
use crate::web::state::broadcast_event;

/// Prefix for structured file attachment payloads in SSE / history `content`.
pub const WEBUI_FILE_EVENT_PREFIX: &str = "__ccg_file__:";

static MEDIA_FILENAME_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\.[a-z0-9]{1,12}$").expect("media filename regex"));

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WebUiFileTarget {
    pub session_id: String,
}

pub fn mcp_context_for_session(session_id: &str) -> McpContext {
    McpContext {
        delivery: McpDeliveryTarget::WebUi(WebUiFileTarget {
            session_id: session_id.to_string(),
        }),
    }
}

pub fn build_file_event_content(
    media_filename: &str,
    display_name: &str,
    size: u64,
    is_image: bool,
) -> String {
    let payload = json!({
        "v": 1,
        "kind": "file",
        "media": media_filename,
        "name": display_name,
        "size": size,
        "is_image": is_image,
    });
    format!("{WEBUI_FILE_EVENT_PREFIX}{payload}")
}

pub fn broadcast_file_attachment(
    session_id: &str,
    role: &str,
    media_filename: &str,
    display_name: &str,
    size: u64,
    is_image: bool,
) {
    let content = build_file_event_content(media_filename, display_name, size, is_image);
    broadcast_event(session_id, "webui", session_id, role, &content);
}

/// Resolve `~/.cc-gateway/media/<storage_name>` after validating the storage basename.
pub fn resolve_media_path(storage_name: &str) -> Result<PathBuf> {
    if !MEDIA_FILENAME_RE.is_match(storage_name) {
        anyhow::bail!("Invalid media file name");
    }
    let dir = media_dir();
    let path = dir.join(storage_name);
    let canonical = path.canonicalize().with_context(|| {
        format!("Media file not found: {}", storage_name)
    })?;
    let dir_canon = dir.canonicalize().unwrap_or(dir);
    if !canonical.starts_with(&dir_canon) {
        anyhow::bail!("Media path escapes media directory");
    }
    Ok(canonical)
}

pub async fn save_upload_for_webui(
    bytes: &[u8],
    original_name: &str,
    content_type: Option<&str>,
) -> Result<SavedInboundMedia> {
    if bytes.is_empty() {
        anyhow::bail!("Empty file");
    }
    if bytes.len() as u64 > MAX_OUTBOUND_FILE_BYTES {
        anyhow::bail!(
            "File too large: {} bytes (max {}MB)",
            bytes.len(),
            MAX_OUTBOUND_FILE_BYTES / 1024 / 1024
        );
    }
    let name = sanitize_upload_filename(original_name);
    save_bytes_to_media_dir_with_upstream_name(bytes, &name, content_type).await
}

fn sanitize_upload_filename(name: &str) -> String {
    let base = Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let safe: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .take(120)
        .collect();
    if safe.is_empty() {
        "file.bin".to_string()
    } else {
        safe
    }
}

/// Listen for `/api/deliver` requests targeting WebUI agent session ids.
pub fn spawn_webui_deliver_listener() -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut rx = crate::web::state::DELIVER_BUS.subscribe();
        loop {
            let Ok(req) = rx.recv().await else {
                continue;
            };
            if crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                .get_agent_session(&req.session_id)
                .is_none()
            {
                continue;
            }
            let session_id = req.session_id.clone();
            let path = req.path.clone();
            let message = req.message.clone();
            tokio::spawn(async move {
                let target = WebUiFileTarget {
                    session_id: session_id.clone(),
                };
                match crate::runtime::file_delivery::validate_outbound_file(&path, None).await {
                    Ok(outbound) => {
                        if let Err(e) = target.send_file(outbound).await {
                            tracing::warn!("[WebUI] deliver send_file failed: {e}");
                        } else if let Some(msg) = message {
                            let trimmed = msg.trim();
                            if !trimmed.is_empty() {
                                broadcast_event(
                                    &session_id,
                                    "webui",
                                    &session_id,
                                    "assistant",
                                    trimmed,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("[WebUI] deliver validate failed: {e}");
                    }
                }
            });
        }
    })
}

pub fn media_storage_basename(path: &Path) -> Result<String> {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .context("Media path has no file name")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_filename_regex_accepts_uuid_ext() {
        assert!(MEDIA_FILENAME_RE.is_match(
            "550e8400-e29b-41d4-a716-446655440000.png"
        ));
        assert!(!MEDIA_FILENAME_RE.is_match("../etc/passwd"));
    }

    #[test]
    fn file_event_content_has_prefix() {
        let s = build_file_event_content(
            "550e8400-e29b-41d4-a716-446655440000.pdf",
            "a.pdf",
            9,
            false,
        );
        let img = build_file_event_content(
            "550e8400-e29b-41d4-a716-446655440000.png",
            "shot.png",
            9,
            true,
        );
        assert!(img.contains("\"is_image\":true"));
        assert!(s.starts_with(WEBUI_FILE_EVENT_PREFIX));
        assert!(s.contains("a.pdf"));
    }
}
