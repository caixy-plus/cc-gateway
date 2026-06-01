use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;

const INSTALL_SH_URL: &str =
    "https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.sh";
const INSTALL_PS1_URL: &str =
    "https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.ps1";
const SKIP_SETUP_ENV: &str = "CC_GATEWAY_SKIP_SETUP";

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

/// Fetch commit messages between two refs from GitHub compare API.
pub async fn fetch_compare_commits(
    client: &reqwest::Client,
    repo: &str,
    base: &str,
    head: &str,
) -> Result<Vec<String>> {
    let url = format!(
        "https://api.github.com/repos/{}/compare/{}...{}",
        repo, base, head
    );
    let resp = client
        .get(&url)
        .header("User-Agent", "cc-gateway-updater")
        .send()
        .await
        .context("failed to fetch compare from GitHub API")?;

    if !resp.status().is_success() {
        anyhow::bail!("GitHub compare API returned status: {}", resp.status());
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .context("failed to parse compare response")?;

    let commits: Vec<String> = json["commits"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["commit"]["message"].as_str().map(|m| m.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(commits)
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
    let _url = build_download_url(repo, &release.tag_name, &platform);

    Ok(Some(ReleaseInfo {
        tag_name: release.tag_name,
        body: release.body.unwrap_or_default(),
    }))
}

#[allow(dead_code)]
fn binary_name_for_current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "cc-gateway.exe"
    } else {
        "cc-gateway"
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
fn update_tmp_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("cc-gateway");
    target.with_file_name(format!(".{}.update-tmp", file_name))
}

#[cfg(any(windows, test))]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn local_codesign(path: &Path) {
    let Ok(status) = std::process::Command::new("codesign")
        .arg("-s")
        .arg("-")
        .arg("-f")
        .arg(path)
        .status()
    else {
        eprintln!("Warning: local codesign command failed to start");
        return;
    };

    if !status.success() {
        eprintln!("Warning: local codesign failed, keeping downloaded binary signature");
    }
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn clear_macos_xattrs(path: &Path) {
    let _ = std::process::Command::new("xattr")
        .arg("-cr")
        .arg(path)
        .status();
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn replace_binary(tmp_path: &Path, target: &Path) -> Result<()> {
    let _ = std::fs::remove_file(target);
    std::fs::copy(tmp_path, target).context("failed to copy binary into place")?;
    make_executable(target)?;
    clear_macos_xattrs(target);
    std::fs::remove_file(tmp_path).context("failed to remove temp file")?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn replace_binary(tmp_path: &Path, target: &Path) -> Result<()> {
    std::fs::rename(tmp_path, target).context("failed to replace binary")?;
    Ok(())
}

#[cfg(windows)]
fn schedule_windows_self_replace(
    tmp_path: &Path,
    target: &Path,
    restart_daemon: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let updater_pid = std::process::id();
    let tmp = powershell_quote(&tmp_path.to_string_lossy());
    let target_quoted = powershell_quote(&target.to_string_lossy());
    let restart = if restart_daemon { "$true" } else { "$false" };
    let restart_args = match config_path {
        Some(path) => format!(
            "@('start','--config',{})",
            powershell_quote(&path.to_string_lossy())
        ),
        None => "@('start')".to_string(),
    };
    let script = format!(
        r#"$ErrorActionPreference = 'Stop'; $pidToWait = {updater_pid}; $tmp = {tmp}; $target = {target}; $restart = {restart}; $restartArgs = {restart_args}; while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; $moved = $false; for ($i = 0; $i -lt 120; $i++) {{ try {{ Move-Item -LiteralPath $tmp -Destination $target -Force; $moved = $true; break }} catch {{ Start-Sleep -Milliseconds 500 }} }}; if (-not $moved) {{ throw "failed to replace binary after waiting for locks" }}; if ($restart) {{ Start-Process -FilePath $target -ArgumentList $restartArgs -WindowStyle Hidden }}"#,
        updater_pid = updater_pid,
        tmp = tmp,
        target = target_quoted,
        restart = restart,
        restart_args = restart_args
    );

    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-Command",
            &script,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to schedule Windows binary replacement")?;

    Ok(())
}

#[allow(dead_code)]
pub(crate) fn install_downloaded_binary(target: &Path, binary: &[u8]) -> Result<()> {
    let tmp_path = update_tmp_path(target);
    std::fs::write(&tmp_path, binary).context("failed to write temp file")?;
    make_executable(&tmp_path)?;

    #[cfg(target_os = "macos")]
    local_codesign(&tmp_path);

    replace_binary(&tmp_path, target)?;
    Ok(())
}

#[allow(dead_code)]
async fn download_binary(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("failed to download binary")?;

    if !resp.status().is_success() {
        anyhow::bail!("download returned status: {}", resp.status());
    }

    let bytes = resp.bytes().await.context("failed to read download body")?;
    binary_bytes_from_download(url, &bytes)
}

/// Download a binary to a temporary path and atomically replace the target.
#[allow(dead_code)]
pub async fn download_and_replace(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
) -> Result<()> {
    let binary = download_binary(client, url).await?;
    install_downloaded_binary(target, &binary)?;

    Ok(())
}

#[cfg(windows)]
async fn download_and_schedule_windows_replace(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    restart_daemon: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    let binary = download_binary(client, url).await?;
    let tmp_path = update_tmp_path(target);
    std::fs::write(&tmp_path, binary).context("failed to write temp file")?;
    schedule_windows_self_replace(&tmp_path, target, restart_daemon, config_path)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn default_install_dir() -> String {
    if cfg!(target_os = "windows") {
        std::env::var("LOCALAPPDATA")
            .map(|d| format!(r"{}\cc-gateway", d))
            .unwrap_or_else(|_| r"%LOCALAPPDATA%\cc-gateway".to_string())
    } else {
        std::env::var("INSTALL_DIR").unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|h| format!("{}/.local/bin", h.display()))
                .unwrap_or_else(|| "$HOME/.local/bin".to_string())
        })
    }
}

async fn daemon_was_running() -> bool {
    let Ok(config_dir) = crate::config::loader::ConfigLoader::ensure_config_dir() else {
        return false;
    };
    let pid_file = config_dir.join("daemon.pid");
    let Ok(pid_str) = std::fs::read_to_string(&pid_file) else {
        return false;
    };
    let Ok(pid) = pid_str.trim().parse::<u32>() else {
        return false;
    };
    crate::daemon::is_process_alive(pid)
}

#[cfg(unix)]
fn schedule_install_script_and_exit(
    restart_daemon: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    // Run installer in the current terminal so the user can see progress.
    // On Unix, overwriting the running executable is allowed (inode stays alive
    // until process exits), so we don't need to detach.
    let status = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "curl -fsSL {} | sh",
            shell_single_quote(INSTALL_SH_URL)
        ))
        .env(SKIP_SETUP_ENV, "1")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to run install script")?;

    if !status.success() {
        anyhow::bail!("install script failed with exit code: {:?}", status.code());
    }

    if restart_daemon {
        let gateway = PathBuf::from(default_install_dir()).join("cc-gateway");
        if gateway.exists() {
            let mut cmd = std::process::Command::new(&gateway);
            cmd.arg("restart");
            if let Some(path) = config_path {
                cmd.arg("--config").arg(path);
            }
            let status = cmd
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status()
                .context("failed to restart daemon after update")?;
            if !status.success() {
                anyhow::bail!("failed to restart daemon after update");
            }
        }
    }

    Ok(())
}

#[cfg(windows)]
fn schedule_install_script_and_exit(
    restart_daemon: bool,
    config_path: Option<&Path>,
) -> Result<()> {
    // Windows cannot replace the running executable; keep the detached updater
    // style, but DO NOT hide/redirect output so the user sees progress.
    let updater_pid = std::process::id();
    let gateway = format!(r"{}\cc-gateway.exe", default_install_dir());
    let installer_path = std::env::temp_dir().join("cc-gateway-install.ps1");
    let script = build_windows_update_install_script(
        updater_pid,
        restart_daemon,
        config_path,
        &gateway,
        &installer_path.to_string_lossy(),
    );

    std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn Windows update install script")?;

    println!("Updater will exit; installation continues in the background.");
    Ok(())
}

#[cfg(any(windows, test))]
pub(crate) fn build_windows_update_install_script(
    updater_pid: u32,
    restart_daemon: bool,
    config_path: Option<&Path>,
    gateway: &str,
    installer_path: &str,
) -> String {
    let restart = if restart_daemon { "$true" } else { "$false" };
    let restart_args = match config_path {
        Some(path) => format!(
            "@('restart','--config',{})",
            powershell_quote(&path.to_string_lossy())
        ),
        None => "@('restart')".to_string(),
    };
    format!(
        r#"$ErrorActionPreference = 'Stop'; $pidToWait = {updater_pid}; $restart = {restart}; $gateway = {gateway}; $installer = {installer}; $restartArgs = {restart_args}; while (Get-Process -Id $pidToWait -ErrorAction SilentlyContinue) {{ Start-Sleep -Milliseconds 200 }}; try {{ $env:{skip_env} = '1'; Invoke-WebRequest -UseBasicParsing -Uri {install_url} -OutFile $installer; & $installer; if (-not $?) {{ throw "install script failed" }}; if ($restart) {{ & $gateway @restartArgs }} }} finally {{ Remove-Item -LiteralPath $installer -ErrorAction SilentlyContinue; Remove-Item Env:\{skip_env} -ErrorAction SilentlyContinue }}"#,
        updater_pid = updater_pid,
        restart = restart,
        gateway = powershell_quote(gateway),
        installer = powershell_quote(installer_path),
        restart_args = restart_args,
        skip_env = SKIP_SETUP_ENV,
        install_url = powershell_quote(INSTALL_PS1_URL),
    )
}

/// CLI entry point for `cc-gateway update`.
pub async fn run(
    check_only: bool,
    force: bool,
    yes: bool,
    config_path: Option<PathBuf>,
) -> Result<()> {
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
                let _url = build_download_url(repo, &release.tag_name, &platform);
                ReleaseInfo {
                    tag_name: release.tag_name.clone(),
                    body: release.body.unwrap_or_default(),
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
    println!("Install script: {}", install_script_url());

    if check_only {
        println!("\nUse `cc-gateway update` without --check to install.");
        return Ok(());
    }

    if !yes {
        print!("Download and install update? [y/N] ");
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Aborted.");
            return Ok(());
        }
    }

    let restart_daemon = daemon_was_running().await;
    println!("Stopping cc-gateway daemon...");
    let _ = crate::daemon::stop().await;

    println!("Installing via official install script...");
    schedule_install_script_and_exit(restart_daemon, config_path.as_deref())?;
    std::process::exit(0);
}

fn install_script_url() -> &'static str {
    if cfg!(target_os = "windows") {
        INSTALL_PS1_URL
    } else {
        INSTALL_SH_URL
    }
}
