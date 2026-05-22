use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::path::Path;

/// A parsed semantic version (major.minor.patch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim_start_matches('v');
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            anyhow::bail!("expected semver format X.Y.Z, got: {}", s);
        }
        Ok(Version {
            major: parts[0].parse().context("invalid major version")?,
            minor: parts[1].parse().context("invalid minor version")?,
            patch: parts[2].parse().context("invalid patch version")?,
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
    }
}

/// Information about a GitHub release.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub body: Option<String>,
    pub html_url: String,
}

/// Parsed release info useful for the updater.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: Version,
    pub tag_name: String,
    pub body: String,
    pub url: String,
}

/// Parse a GitHub release JSON payload.
pub fn parse_release_json(json: &str) -> Result<GitHubRelease> {
    serde_json::from_str(json).context("failed to parse GitHub release JSON")
}

/// Compare current and latest versions.
pub fn needs_update(current: &str, latest: &str) -> Result<bool> {
    let current = Version::parse(current)?;
    let latest = Version::parse(latest)?;
    Ok(latest > current)
}

/// Detect the platform name used in release asset filenames.
pub fn detect_platform() -> String {
    let os = if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };
    let arch = if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "unknown"
    };
    let ext = if cfg!(target_os = "windows") { ".exe" } else { "" };
    format!("{}-{}{}", os, arch, ext)
}

/// Build the download URL for a given release tag and platform.
pub fn build_download_url(repo: &str, tag: &str, platform: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/{}/cc-gateway-{}",
        repo, tag, platform
    )
}

/// Fetch the latest release from GitHub.
pub async fn fetch_latest_release(client: &reqwest::Client, repo: &str) -> Result<GitHubRelease> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let resp = client
        .get(&url)
        .header("User-Agent", "cc-gateway-updater")
        .send()
        .await
        .context("failed to send request to GitHub API")?;

    if !resp.status().is_success() {
        anyhow::bail!("GitHub API returned status: {}", resp.status());
    }

    let json = resp.text().await.context("failed to read GitHub response")?;
    parse_release_json(&json)
}

/// Check if an update is available and return release info.
pub async fn check_update(
    client: &reqwest::Client,
    repo: &str,
    current_version: &str,
) -> Result<Option<ReleaseInfo>> {
    let release = fetch_latest_release(client, repo).await?;
    let latest = Version::parse(&release.tag_name)?;
    let current = Version::parse(current_version)?;

    if latest <= current {
        return Ok(None);
    }

    let platform = detect_platform();
    let url = build_download_url(repo, &release.tag_name, &platform);

    Ok(Some(ReleaseInfo {
        version: latest,
        tag_name: release.tag_name,
        body: release.body.unwrap_or_default(),
        url,
    }))
}

/// Download a binary to a temporary path and atomically replace the target.
pub async fn download_and_replace(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
) -> Result<()> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("failed to download binary")?;

    if !resp.status().is_success() {
        anyhow::bail!("download returned status: {}", resp.status());
    }

    let bytes = resp.bytes().await.context("failed to read download body")?;

    let tmp_path = target.with_extension("tmp");
    std::fs::write(&tmp_path, &bytes).context("failed to write temp file")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&tmp_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&tmp_path, perms)?;
    }

    std::fs::rename(&tmp_path, target).context("failed to replace binary")?;

    Ok(())
}

/// CLI entry point for `cc-gateway update`.
pub async fn run(check_only: bool, force: bool) -> Result<()> {
    let repo = "caixy-plus/cc-gateway";
    let current = env!("CARGO_PKG_VERSION");

    println!("Current version: {}", current);
    println!("Checking for updates...");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;

    let info = match check_update(&client, repo, current).await? {
        Some(info) => info,
        None => {
            if force {
                println!("Already on latest version, but --force was set.");
                // We still need release info to get the download URL.
                let release = fetch_latest_release(&client, repo).await?;
                let platform = detect_platform();
                let url = build_download_url(repo, &release.tag_name, &platform);
                ReleaseInfo {
                    version: Version::parse(&release.tag_name)?,
                    tag_name: release.tag_name.clone(),
                    body: release.body.unwrap_or_default(),
                    url,
                }
            } else {
                println!("Already on the latest version.");
                return Ok(());
            }
        }
    };

    println!(
        "New version available: {} (current: {})",
        info.tag_name, current
    );
    println!("Release notes:");
    println!("{}", info.body);
    println!("Download URL: {}", info.url);

    if check_only {
        println!("\nUse `cc-gateway update` without --check to install.");
        return Ok(());
    }

    let exe = std::env::current_exe().context("failed to get current executable path")?;
    println!("Target binary: {}", exe.display());

    print!("Download and replace? [y/N] ");
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Aborted.");
        return Ok(());
    }

    println!("Downloading...");
    download_and_replace(&client, &info.url, &exe)
        .await
        .context("failed to download and replace binary")?;

    println!("Binary updated. Restarting daemon...");
    crate::daemon::restart(None).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_needs_update() {
        assert!(needs_update("1.0.0", "1.0.1").unwrap());
        assert!(needs_update("1.0.0", "1.1.0").unwrap());
        assert!(needs_update("1.0.0", "2.0.0").unwrap());
        assert!(!needs_update("1.0.0", "1.0.0").unwrap());
        assert!(!needs_update("1.0.1", "1.0.0").unwrap());
        assert!(!needs_update("1.0.0", "v1.0.0").unwrap());
    }

    #[test]
    fn test_parse_release_json() {
        let json = serde_json::json!({
            "tag_name": "v1.1.0",
            "body": "## What is new\n- feature A\n- bugfix B",
            "html_url": "https://github.com/caixy-plus/cc-gateway/releases/tag/v1.1.0"
        }).to_string();
        let release = parse_release_json(&json).unwrap();
        assert_eq!(release.tag_name, "v1.1.0");
        assert_eq!(release.body.as_deref().unwrap(), "## What is new\n- feature A\n- bugfix B");
        assert_eq!(release.html_url, "https://github.com/caixy-plus/cc-gateway/releases/tag/v1.1.0");
    }

    #[test]
    fn test_detect_platform_not_empty() {
        let platform = detect_platform();
        assert!(!platform.is_empty());
        assert!(!platform.contains("unknown"));
    }

    #[test]
    fn test_build_download_url() {
        let url = build_download_url("caixy-plus/cc-gateway", "v1.1.0", "darwin-arm64");
        assert_eq!(
            url,
            "https://github.com/caixy-plus/cc-gateway/releases/download/v1.1.0/cc-gateway-darwin-arm64"
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

        download_and_replace(&client, &url, &target_path)
            .await
            .unwrap();

        let content = std::fs::read(&target_path).unwrap();
        assert_eq!(content, b"ABCD");

        let _ = std::fs::remove_file(&target_path);
    }
}
