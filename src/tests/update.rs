use crate::update::{build_download_url, detect_platform, parse_release_json, Version};

#[test]
fn test_version_parse_basic() {
    let v = Version::parse("1.2.3").unwrap();
    assert_eq!(v.major, 1);
    assert_eq!(v.minor, 2);
    assert_eq!(v.patch, 3);
}

#[test]
fn test_version_parse_with_v_prefix() {
    let v = Version::parse("v2.0.0").unwrap();
    assert_eq!(v.major, 2);
    assert_eq!(v.minor, 0);
    assert_eq!(v.patch, 0);
}

#[test]
fn test_version_parse_invalid() {
    assert!(Version::parse("1.2").is_err());
    assert!(Version::parse("abc").is_err());
    assert!(Version::parse("").is_err());
}

#[test]
fn test_version_ordering() {
    assert!(Version::parse("1.0.0").unwrap() < Version::parse("1.0.1").unwrap());
    assert!(Version::parse("1.0.0").unwrap() < Version::parse("1.1.0").unwrap());
    assert!(Version::parse("1.0.0").unwrap() < Version::parse("2.0.0").unwrap());
    assert!(Version::parse("1.0.10").unwrap() > Version::parse("1.0.2").unwrap());
    assert!(Version::parse("1.0.0").unwrap() == Version::parse("1.0.0").unwrap());
}

#[test]
fn test_parse_release_json() {
    let json = serde_json::json!({
        "tag_name": "v1.1.0",
        "body": "## What is new\n- feature A\n- bugfix B",
        "html_url": "https://github.com/caixy-plus/cc-gateway/releases/tag/v1.1.0"
    })
    .to_string();
    let release = parse_release_json(&json).unwrap();
    assert_eq!(release.tag_name, "v1.1.0");
    assert_eq!(
        release.body.as_deref().unwrap(),
        "## What is new\n- feature A\n- bugfix B"
    );
    assert_eq!(
        release.html_url.as_deref(),
        Some("https://github.com/caixy-plus/cc-gateway/releases/tag/v1.1.0")
    );
}

#[test]
fn test_detect_platform_not_empty() {
    let platform = detect_platform();
    assert!(!platform.is_empty());
    assert!(!platform.contains("unknown"));
    assert!(platform.ends_with(".tar.gz") || platform.ends_with(".zip"));
}

#[test]
fn test_build_download_url() {
    let url = build_download_url(
        "caixy-plus/cc-gateway",
        "v1.1.0",
        "aarch64-apple-darwin.tar.gz",
    );
    assert_eq!(
        url,
        "https://github.com/caixy-plus/cc-gateway/releases/download/v1.1.0/cc-gateway-aarch64-apple-darwin.tar.gz"
    );
}

#[tokio::test]
async fn test_download_and_replace() {
    // Start a tiny HTTP server that serves a fixed binary payload.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nABCD";
            let _ = tokio::io::AsyncWriteExt::write_all(&mut socket, response).await;
        }
    });

    let client = reqwest::Client::new();
    let url = format!("http://{}/download", addr);
    let dir = std::env::temp_dir();
    let target_path = dir.join("cc-gateway-test-update-bin");
    let _ = std::fs::remove_file(&target_path);

    crate::update::download_and_replace(&client, &url, &target_path)
        .await
        .unwrap();

    let content = std::fs::read(&target_path).unwrap();
    assert_eq!(content, b"ABCD");

    let _ = std::fs::remove_file(&target_path);
}
