use axum::extract::State;
use axum::http::StatusCode;
use serde_json::json;
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::web::handlers::session::AppState;

const UPDATE_REPO: &str = "caixy-plus/cc-gateway";

#[derive(Debug, Clone, Copy)]
enum DaemonCommand {
    Restart,
    Update,
}

fn build_daemon_command_args(command: DaemonCommand, config_path: Option<&Path>) -> Vec<String> {
    let mut args = match command {
        DaemonCommand::Restart => vec!["restart".to_string()],
        DaemonCommand::Update => vec!["update".to_string(), "--yes".to_string()],
    };
    if let Some(path) = config_path {
        args.push("--config".to_string());
        args.push(path.to_string_lossy().to_string());
    }
    args
}

fn build_update_check_body(
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

pub async fn handle_version() -> impl axum::response::IntoResponse {
    let version = env!("CARGO_PKG_VERSION");
    // Serve as real `application/json` (Axum `Json`) and forbid caching: a JSON
    // body mislabeled `text/plain` with no cache directives can be stale-cached
    // or mangled by Firefox's heuristic cache / intermediary proxies, which makes
    // the WebUI footer render `vunknown`.
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        axum::Json(json!({ "version": version })),
    )
}

/// Returns true when the release body is just an auto-generated "Full Changelog" link.
fn is_auto_changelog(body: &str) -> bool {
    let trimmed = body.trim();
    trimmed.starts_with("**Full Changelog**:") && !trimmed.contains('\n')
}

/// Fetch commit log from compare API and format it as release notes.
async fn fetch_changelog(client: &reqwest::Client, current: &str, latest: &str) -> Option<String> {
    let commits =
        crate::update::fetch_compare_commits(client, UPDATE_REPO, &format!("v{}", current), latest)
            .await
            .ok()?;
    if commits.is_empty() {
        return None;
    }
    Some(
        commits
            .iter()
            .map(|c| {
                // Take just the first line (subject) of each commit message
                let subject = c.lines().next().unwrap_or(c);
                format!("  • {}", subject)
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
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

    // If the release body is just the auto-generated link, fetch actual commits.
    let mut release_with_notes = release;
    if is_auto_changelog(&release_with_notes.body.clone().unwrap_or_default()) {
        if let Some(changelog) =
            fetch_changelog(&client, current, &release_with_notes.tag_name).await
        {
            release_with_notes.body = Some(changelog);
        }
    }

    let body = match build_update_check_body(current, release_with_notes) {
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

pub async fn handle_restart(State(state): State<AppState>) -> (StatusCode, String) {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            let body = json!({ "error": format!("Failed to determine executable path: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    // Spawn a detached child that waits a moment then restarts the daemon.
    // The parent daemon will exit before the child runs, so the restart works.
    let args =
        build_daemon_command_args(DaemonCommand::Restart, state.daemon_config_path.as_deref());
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::core::agent::no_console_window(&mut cmd);
    if let Err(e) = cmd.spawn() {
        let body = json!({ "error": format!("Failed to spawn restart command: {}", e) });
        return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
    }

    let body = json!({
        "status": "restarting",
        "command": format!("{} {}", exe.display(), args.join(" "))
    });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_update(State(state): State<AppState>) -> (StatusCode, String) {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            let body = json!({ "error": format!("Failed to determine executable path: {}", e) });
            return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
        }
    };

    let args =
        build_daemon_command_args(DaemonCommand::Update, state.daemon_config_path.as_deref());
    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::core::agent::no_console_window(&mut cmd);
    if let Err(e) = cmd.spawn() {
        let body = json!({ "error": format!("Failed to spawn update command: {}", e) });
        return (StatusCode::INTERNAL_SERVER_ERROR, body.to_string());
    }

    let body = json!({
        "status": "updating",
        "command": format!("{} {}", exe.display(), args.join(" "))
    });
    (StatusCode::ACCEPTED, body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::GitHubRelease;
    use std::path::Path;

    #[test]
    fn update_check_reports_up_to_date_as_success() {
        let body = build_update_check_body(
            "1.5.0",
            GitHubRelease {
                tag_name: "v1.5.0".to_string(),
                body: Some("No changes".to_string()),
                html_url: None,
            },
        )
        .unwrap();

        assert_eq!(body["status"], "up_to_date");
        assert_eq!(body["update_available"], false);
        assert_eq!(body["current_version"], "1.5.0");
        assert_eq!(body["latest_version"], "v1.5.0");
    }

    #[test]
    fn update_check_reports_available_when_latest_is_newer() {
        let body = build_update_check_body(
            "1.5.0",
            GitHubRelease {
                tag_name: "v1.6.0".to_string(),
                body: Some("New release".to_string()),
                html_url: None,
            },
        )
        .unwrap();

        assert_eq!(body["status"], "available");
        assert_eq!(body["update_available"], true);
        assert_eq!(body["release_notes"], "New release");
    }

    #[test]
    fn restart_command_preserves_explicit_config_path() {
        let args = build_daemon_command_args(
            DaemonCommand::Restart,
            Some(Path::new("/tmp/cc-gateway/custom.json")),
        );

        assert_eq!(
            args,
            vec!["restart", "--config", "/tmp/cc-gateway/custom.json"]
        );
    }

    #[test]
    fn update_command_preserves_explicit_config_path() {
        let args = build_daemon_command_args(
            DaemonCommand::Update,
            Some(Path::new("/tmp/cc-gateway/custom.json")),
        );

        assert_eq!(
            args,
            vec!["update", "--yes", "--config", "/tmp/cc-gateway/custom.json"]
        );
    }

    // The WebUI footer renders the version as `v{version}`; when the JSON body is
    // mislabeled `text/plain` or served stale from a cache/proxy, the frontend
    // parse yields an empty object and the footer shows `vunknown` (notably in
    // Firefox's heuristic cache). Guard the content-type and cache directives.
    #[tokio::test]
    async fn version_response_is_json_and_not_cached() {
        use axum::body::to_bytes;
        use axum::response::IntoResponse;

        let resp = handle_version().await.into_response();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
        assert_eq!(
            resp.headers()
                .get(axum::http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("no-store")
        );

        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["version"], env!("CARGO_PKG_VERSION"));
    }
}
