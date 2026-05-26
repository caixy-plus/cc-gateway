use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io::{Cursor, Read};
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
    #[serde(default)]
    #[allow(dead_code)]
    pub html_url: Option<String>,
}

/// Parsed release info useful for the updater.
#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub body: String,
    pub url: String,
}

/// Parse a GitHub release JSON payload.
pub fn parse_release_json(json: &str) -> Result<GitHubRelease> {
    serde_json::from_str(json).context("failed to parse GitHub release JSON")
}

/// Detect the platform name used in release asset filenames.
pub fn detect_platform() -> String {
    let target = if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "aarch64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else {
        "unknown"
    };
    let archive_ext = if cfg!(target_os = "windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("{}.{}", target, archive_ext)
}

/// Compare two version strings and return true if `latest` is newer than `current`.
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

    let json = resp
        .text()
        .await
        .context("failed to read GitHub response")?;
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
        tag_name: release.tag_name,
        body: release.body.unwrap_or_default(),
        url,
    }))
}

fn binary_name_for_current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "cc-gateway.exe"
    } else {
        "cc-gateway"
    }
}

fn extract_binary_from_tar_gz(bytes: &[u8], binary_name: &str) -> Result<Vec<u8>> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);

    for entry in archive.entries().context("failed to read tar archive")? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to read tar entry path")?;
        if path.file_name() != Some(OsStr::new(binary_name)) {
            continue;
        }

        let mut binary = Vec::new();
        entry
            .read_to_end(&mut binary)
            .context("failed to extract binary from tar archive")?;
        return Ok(binary);
    }

    anyhow::bail!("archive does not contain {}", binary_name);
}

fn extract_binary_from_zip(bytes: &[u8], binary_name: &str) -> Result<Vec<u8>> {
    let reader = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).context("failed to read zip archive")?;

    for idx in 0..archive.len() {
        let mut file = archive
            .by_index(idx)
            .context("failed to read zip archive entry")?;
        let Some(name) = file.name().rsplit('/').next() else {
            continue;
        };
        if name != binary_name {
            continue;
        }

        let mut binary = Vec::new();
        file.read_to_end(&mut binary)
            .context("failed to extract binary from zip archive")?;
        return Ok(binary);
    }

    anyhow::bail!("archive does not contain {}", binary_name);
}

fn binary_bytes_from_download(url: &str, bytes: &[u8]) -> Result<Vec<u8>> {
    let binary_name = binary_name_for_current_platform();
    if url.ends_with(".tar.gz") {
        extract_binary_from_tar_gz(bytes, binary_name)
    } else if url.ends_with(".zip") {
        extract_binary_from_zip(bytes, binary_name)
    } else {
        Ok(bytes.to_vec())
    }
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
    let binary = binary_bytes_from_download(url, &bytes)?;

    let tmp_path = target.with_extension("tmp");
    std::fs::write(&tmp_path, &binary).context("failed to write temp file")?;

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
