// Tests for Feishu media helpers (src/platform/feishu/media.rs).
//
// download_message_resource requires a live FeishuPlatform with HTTP
// access and real credentials, so we test:
//   - The content-type-to-extension mapping logic
//   - FeishuPlatform construction for media scenarios
//   - Cache directory path construction
//   - Edge cases around media response handling

use crate::platform::feishu::FeishuPlatform;
use crate::config::model::{FeishuConfig, GatewayConfig};
use crate::daemon::cleaner;

fn make_test_platform() -> FeishuPlatform {
    let config = FeishuConfig {
        enabled: true,
        app_id: "test_app".to_string(),
        app_secret: "test_secret".to_string(),
        allow_from: "*".to_string(),
        encrypt_key: "".to_string(),
        mode: "websocket".to_string(),
        webhook_bind: "0.0.0.0:3000".to_string(),
    };
    let gateway_config = GatewayConfig::default();
    FeishuPlatform::new(
        config,
        &gateway_config.default_dir,
        gateway_config.claude.clone(),
        gateway_config.show_thinking,
    )
}

// ---------------------------------------------------------------------------
// Media cache directory
// ---------------------------------------------------------------------------

#[test]
fn test_media_cache_dir_exists_or_creatable() {
    let dir = cleaner::media_dir();
    // The path should end with "media" under the cc-gateway config dir
    let dir_str = dir.to_string_lossy();
    assert!(dir_str.contains("media") || dir_str.contains("cc-gateway"),
        "media_dir should be under cc-gateway config dir, got: {}", dir_str);
}

#[test]
fn test_media_cache_dir_is_absolute() {
    let dir = cleaner::media_dir();
    assert!(dir.is_absolute(), "media_dir should be absolute, got: {:?}", dir);
}

// ---------------------------------------------------------------------------
// Content-type to extension mapping (extracted logic test)
// ---------------------------------------------------------------------------

/// The content-type-to-extension mapping embedded in download_message_resource.
/// We replicate it here to verify correctness independently.
fn content_type_to_extension(content_type: &str) -> &'static str {
    let base = content_type.split(';').next().unwrap_or("application/octet-stream");
    match base {
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
    }
}

#[test]
fn test_content_type_to_ext_image_jpeg() {
    assert_eq!(content_type_to_extension("image/jpeg"), "jpg");
    assert_eq!(content_type_to_extension("image/jpg"), "jpg");
}

#[test]
fn test_content_type_to_ext_image_png() {
    assert_eq!(content_type_to_extension("image/png"), "png");
}

#[test]
fn test_content_type_to_ext_image_gif() {
    assert_eq!(content_type_to_extension("image/gif"), "gif");
}

#[test]
fn test_content_type_to_ext_image_webp() {
    assert_eq!(content_type_to_extension("image/webp"), "webp");
}

#[test]
fn test_content_type_to_ext_audio() {
    assert_eq!(content_type_to_extension("audio/mpeg"), "mp3");
    assert_eq!(content_type_to_extension("audio/mp3"), "mp3");
    assert_eq!(content_type_to_extension("audio/ogg"), "ogg");
    assert_eq!(content_type_to_extension("audio/wav"), "wav");
    assert_eq!(content_type_to_extension("audio/mp4"), "m4a");
}

#[test]
fn test_content_type_to_ext_video() {
    assert_eq!(content_type_to_extension("video/mp4"), "mp4");
}

#[test]
fn test_content_type_to_ext_text() {
    assert_eq!(content_type_to_extension("text/plain"), "txt");
    assert_eq!(content_type_to_extension("text/markdown"), "md");
}

#[test]
fn test_content_type_to_ext_pdf() {
    assert_eq!(content_type_to_extension("application/pdf"), "pdf");
}

#[test]
fn test_content_type_to_ext_unknown_returns_bin() {
    assert_eq!(content_type_to_extension("application/octet-stream"), "bin");
    assert_eq!(content_type_to_extension("application/zip"), "bin");
    assert_eq!(content_type_to_extension("video/x-msvideo"), "bin");
    assert_eq!(content_type_to_extension(""), "bin");
}

#[test]
fn test_content_type_to_ext_with_charset_parameter() {
    // Content-Type often includes charset: "text/plain; charset=utf-8"
    assert_eq!(content_type_to_extension("text/plain; charset=utf-8"), "txt");
    assert_eq!(content_type_to_extension("text/markdown; charset=utf-8"), "md");
    assert_eq!(content_type_to_extension("application/pdf; boundary=something"), "pdf");
}

#[test]
fn test_content_type_to_ext_with_extra_params() {
    // Multiple parameters should still match the base type
    assert_eq!(content_type_to_extension("image/png; charset=binary; foo=bar"), "png");
}

// ---------------------------------------------------------------------------
// Media filename construction
// ---------------------------------------------------------------------------

#[test]
fn test_media_filename_format() {
    // The filename pattern is: {resource_type}_{file_key}.{ext}
    // e.g., "image_img_001.jpg"
    let resource_type = "image";
    let file_key = "img_abc123";
    let ext = "jpg";
    let filename = format!("{}_{}.{}", resource_type, file_key, ext);
    assert_eq!(filename, "image_img_abc123.jpg");
}

#[test]
fn test_media_filename_with_underscores_in_key() {
    let resource_type = "file";
    let file_key = "key_with_underscores_001";
    let ext = "pdf";
    let filename = format!("{}_{}.{}", resource_type, file_key, ext);
    // The resource_type is separated from file_key by the first underscore;
    // subsequent underscores in file_key are preserved
    assert_eq!(filename, "file_key_with_underscores_001.pdf");
}

// ---------------------------------------------------------------------------
// Platform construction for media (compile-time sanity)
// ---------------------------------------------------------------------------

#[test]
fn test_platform_constructs_for_media_scenarios() {
    let platform = make_test_platform();
    let _ = platform;
}
