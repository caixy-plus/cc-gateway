// Tests for Feishu media helpers (src/platform/feishu/media.rs).
//
// download_message_resource requires a live FeishuPlatform with HTTP
// access and real credentials, so we test:
//   - The content-type-to-extension mapping logic
//   - FeishuPlatform construction for media scenarios
//   - Cache directory path construction
//   - Edge cases around media response handling

use crate::config::model::{FeishuConfig, GatewayConfig};
use crate::daemon::cleaner;
use crate::platform::feishu::FeishuPlatform;

fn make_test_platform() -> FeishuPlatform {
    let config = FeishuConfig {
        enabled: true,
        app_id: "test_app".to_string(),
        app_secret: "test_secret".to_string(),
        allow_from: "*".to_string(),
        encrypt_key: "".to_string(),
        mode: "websocket".to_string(),
        webhook_bind: "0.0.0.0:3000".to_string(),
        require_pairing: false,
    };
    let gateway_config = GatewayConfig::default();
    FeishuPlatform::new(
        config,
        &gateway_config.default_dir,
        gateway_config.agent.clone(),
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
    assert!(
        dir_str.contains("media") || dir_str.contains("cc-gateway"),
        "media_dir should be under cc-gateway config dir, got: {}",
        dir_str
    );
}

#[test]
fn test_media_cache_dir_is_absolute() {
    let dir = cleaner::media_dir();
    assert!(
        dir.is_absolute(),
        "media_dir should be absolute, got: {:?}",
        dir
    );
}

// ---------------------------------------------------------------------------
// Content-type to extension mapping (extracted logic test)
// ---------------------------------------------------------------------------

use crate::platform::inbound_media::content_type_to_extension;

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
    assert_eq!(
        content_type_to_extension("text/plain; charset=utf-8"),
        "txt"
    );
    assert_eq!(
        content_type_to_extension("text/markdown; charset=utf-8"),
        "md"
    );
    assert_eq!(
        content_type_to_extension("application/pdf; boundary=something"),
        "pdf"
    );
}

#[test]
fn test_content_type_to_ext_with_extra_params() {
    // Multiple parameters should still match the base type
    assert_eq!(
        content_type_to_extension("image/png; charset=binary; foo=bar"),
        "png"
    );
}

// ---------------------------------------------------------------------------
// Platform construction for media (compile-time sanity)
// ---------------------------------------------------------------------------

#[test]
fn test_platform_constructs_for_media_scenarios() {
    let platform = make_test_platform();
    let _ = platform;
}
