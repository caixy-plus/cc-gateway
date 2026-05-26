use axum::http::StatusCode;
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;

const UPDATE_REPO: &str = "caixy-plus/cc-gateway";

pub(crate) fn build_update_check_body(
    current: &str,
    release: crate::update::GitHubRelease,
) -> Result<serde_json::Value, String> {
    let latest = crate::update::Version::parse(&release.tag_name)
        .map_err(|e| format!("Invalid latest version: {}", e))?;
    let current_version = crate::update::Version::parse(current)
        .map_err(|e| format!("Invalid current version: {}", e))?;

    let update_available = latest > current_version;
    let platform = crate::update::detect_platform();
    let download_url = crate::update::build_download_url(UPDATE_REPO, &release.tag_name, &platform);
    let release_notes = release.body.unwrap_or_default();
    Ok(json!({
        "status": if update_available { "available" } else { "up_to_date" },
        "update_available": update_available,
        "has_update": update_available,
        "current": current,
        "current_version": current,
        "latest": release.tag_name.clone(),
        "latest_version": release.tag_name,
        "release_notes": release_notes.clone(),
        "body": release_notes,
        "download_url": download_url.clone(),
        "url": download_url,
    }))
}

pub async fn handle_version() -> (StatusCode, String) {
    let version = env!("CARGO_PKG_VERSION");
    let body = json!({ "version": version });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_update_check() -> (StatusCode, String) {
    let current = env!("CARGO_PKG_VERSION");
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            let body = json!({ "error": format!("Failed to build HTTP client: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    let release = match crate::update::fetch_latest_release(&client, UPDATE_REPO).await {
        Ok(release) => release,
        Err(e) => {
            let body = json!({ "error": format!("Failed to check update: {}", e) });
            return (StatusCode::BAD_GATEWAY, body.to_string());
        }
    };

    let body = match build_update_check_body(current, release) {
        Ok(body) => body,
        Err(e) => {
            let status = if e.starts_with("Invalid latest version") {
                StatusCode::BAD_GATEWAY
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            let body = json!({ "error": e });
            return (status, body.to_string());
        }
    };
    (StatusCode::OK, body.to_string())
}

pub async fn handle_restart() -> (StatusCode, String) {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            let body = json!({ "error": format!("Failed to determine executable path: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    // Spawn a detached child that waits a moment then restarts the daemon.
    // The parent daemon will exit before the child runs, so the restart works.
    if let Err(e) = Command::new(&exe)
        .args(["restart"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn()
    {
        let body = json!({ "error": format!("Failed to spawn restart command: {}", e) });
        return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
    }

    let body = json!({
        "status": "restarting",
        "command": format!("{} restart", exe.display())
    });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_update() -> (StatusCode, String) {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            let body = json!({ "error": format!("Failed to determine executable path: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    if let Err(e) = Command::new(&exe)
        .args(["update", "--yes"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn()
    {
        let body = json!({ "error": format!("Failed to spawn update command: {}", e) });
        return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
    }

    let body = json!({
        "status": "updating",
        "command": format!("{} update --yes", exe.display())
    });
    (StatusCode::ACCEPTED, body.to_string())
}
