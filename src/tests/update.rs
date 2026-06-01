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

#[test]
fn windows_update_script_downloads_installer_file_instead_of_iex() {
    let script = crate::update::build_windows_update_install_script(
        1234,
        true,
        Some(std::path::Path::new(r"C:\Users\me\.cc-gateway\config.json")),
        r"C:\Users\me\AppData\Local\cc-gateway\cc-gateway.exe",
        r"C:\Users\me\AppData\Local\Temp\cc-gateway-install.ps1",
    );

    assert!(script.contains("Invoke-WebRequest"));
    assert!(script.contains("-OutFile $installer"));
    assert!(script.contains("& $installer"));
    assert!(script.contains("& $gateway @restartArgs"));
    assert!(script.contains(r"Remove-Item Env:\CC_GATEWAY_SKIP_SETUP"));
    assert!(!script.contains("| iex"));
    assert!(!script.contains("|iex"));
}

#[test]
fn windows_install_skip_setup_returns_to_update_wrapper() {
    let script = include_str!("../../install.ps1");

    assert!(script.contains("if ($env:CC_GATEWAY_SKIP_SETUP)"));
    assert!(script.contains("return"));
    assert!(!script.contains("exit 0"));
}

#[test]
fn windows_install_script_does_not_prefix_empty_user_path_with_separator() {
    let script = include_str!("../../install.ps1");

    assert!(script.contains(r#"$sep = if ($UserPath) { ";" } else { "" }"#));
    assert!(script.contains(r#""$UserPath$sep$InstallDir""#));
    assert!(!script.contains(r#""$UserPath;$InstallDir""#));
}

#[test]
fn windows_uninstall_script_removes_path_entry_with_trailing_slash_variants() {
    let script = include_str!("../../uninstall.ps1");

    assert!(script.contains(r#"$installDirNorm = $InstallDir.TrimEnd('\')"#));
    assert!(script.contains(r#"$_.TrimEnd('\') -ine $installDirNorm"#));
}

#[test]
fn unix_install_script_does_not_write_shell_path_for_default_install_dir() {
    let script = include_str!("../../install.sh");

    assert!(script.contains(r#"DEFAULT_INSTALL_DIR="$HOME/.local/bin""#));
    assert!(script.contains(r#"if [ "$INSTALL_DIR" = "$DEFAULT_INSTALL_DIR" ]; then"#));
    assert!(script.contains("standard user bin path"));
}

#[test]
fn unix_uninstall_script_does_not_modify_shell_path_imports() {
    let script = include_str!("../../uninstall.sh");

    assert!(!script.contains(".zshrc"));
    assert!(!script.contains(".bashrc"));
    assert!(!script.contains("export PATH="));
}

#[tokio::test]
async fn test_download_and_replace() {
    // Start a tiny HTTP server that serves a fixed binary payload.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut buf).await;
            let response = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nABCD";
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

#[cfg(target_os = "macos")]
#[test]
fn test_install_downloaded_binary_applies_macos_install_steps() {
    let dir = tempfile::tempdir().unwrap();
    let target_path = dir.path().join("cc-gateway-test-update-install");
    std::fs::write(&target_path, b"OLD").unwrap();

    let xattr_status = std::process::Command::new("xattr")
        .arg("-w")
        .arg("com.apple.quarantine")
        .arg("0081;00000000;cc-gateway-test;")
        .arg(&target_path)
        .status()
        .unwrap();
    assert!(xattr_status.success());

    crate::update::install_downloaded_binary(&target_path, b"NEW").unwrap();

    let content = std::fs::read(&target_path).unwrap();
    assert_eq!(content, b"NEW");

    let output = std::process::Command::new("xattr")
        .arg("-p")
        .arg("com.apple.quarantine")
        .arg(&target_path)
        .output()
        .unwrap();
    assert!(!output.status.success());
}
