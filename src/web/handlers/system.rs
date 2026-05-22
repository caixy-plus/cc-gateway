use axum::http::StatusCode;
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;

pub async fn handle_version() -> (StatusCode, String) {
    let version = env!("CARGO_PKG_VERSION");
    let body = json!({ "version": version });
    (StatusCode::OK, body.to_string())
}

pub async fn handle_restart() -> (StatusCode, String) {
    let binary_name = if cfg!(windows) {
        "cc-gateway.exe"
    } else {
        "cc-gateway"
    };

    // Spawn a detached child that waits a moment then restarts the daemon.
    // The parent daemon will exit before the child runs, so the restart works.
    let _ = Command::new(binary_name)
        .args(["restart"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false)
        .spawn();

    let body = json!({
        "status": "restarting",
        "command": format!("{} restart", binary_name)
    });
    (StatusCode::OK, body.to_string())
}
