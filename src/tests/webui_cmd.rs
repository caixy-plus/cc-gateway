use axum::extract::Json;
use axum::http::StatusCode;

use crate::db;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
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

#[tokio::test]
async fn webui_cd_updates_active_claude_session_work_dir() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let root = env.home().join("webui-cd-root");
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", root.to_str().unwrap())
        .await
        .unwrap();
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_claude_session_for_platform(
            &runtime.channel_session.id,
            "WebUI cd active",
            root.to_str().unwrap(),
            env.fake_claude_config(),
            false,
            vec![],
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let session_id = active.claude_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&runtime.channel_session.id, Some(active));

    let (status, body) = handle_cd(Json(CdRequest {
        session_id: Some(session_id.clone()),
        path: child.to_string_lossy().to_string(),
    }))
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_claude_session(&session_id)
            .unwrap()
            .work_dir,
        child.to_string_lossy()
    );

    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&runtime.channel_session.id)
        .await
        .unwrap();
}
