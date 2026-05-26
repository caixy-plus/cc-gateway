use crate::update::GitHubRelease;
use crate::web::handlers::system::build_update_check_body;

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
