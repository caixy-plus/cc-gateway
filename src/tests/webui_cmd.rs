use axum::extract::Json;
use axum::http::StatusCode;

use crate::web::handlers::cmd::{handle_cd, CdRequest};

use super::helpers::TestEnv;

#[tokio::test]
async fn webui_cd_rejects_directories_outside_home() {
    let env = TestEnv::new();
    let outside = std::env::current_dir().unwrap();

    let (status, body) = handle_cd(Json(CdRequest {
        session_id: None,
        path: outside.to_string_lossy().to_string(),
    }))
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("Access denied"));
    assert_eq!(std::env::var("HOME").unwrap(), env.home().to_string_lossy());
}
