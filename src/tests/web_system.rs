use crate::update::GitHubRelease;
use crate::web::handlers::system::{
    build_daemon_command_args, build_update_check_body, DaemonCommand,
};
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
