use axum::extract::Json;
use axum::http::StatusCode;

use crate::db;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::handlers::cmd::{handle_cd, handle_pwd, CdRequest, SessionCmdRequest};

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

#[tokio::test]
async fn webui_cd_uses_frontend_session_id_work_dir_for_inactive_session() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let root = env.home().join("webui-cd-selected-root");
    let first = root.join("first");
    let second = root.join("second");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", root.to_str().unwrap())
        .await
        .unwrap();
    let first_session = GLOBAL_CHANNEL_SESSIONS
        .create_claude_session_only(
            &runtime.channel_session.id,
            "First inactive",
            first.to_str().unwrap(),
        )
        .unwrap();
    let second_session = GLOBAL_CHANNEL_SESSIONS
        .create_claude_session_only(
            &runtime.channel_session.id,
            "Second inactive",
            second.to_str().unwrap(),
        )
        .unwrap();

    let (status, body) = handle_pwd(Json(SessionCmdRequest {
        session_id: Some(second_session.id.clone()),
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body).unwrap()["dir"]
            .as_str()
            .unwrap(),
        second.to_str().unwrap()
    );

    let child = second.join("child");
    std::fs::create_dir_all(&child).unwrap();
    let (status, body) = handle_cd(Json(CdRequest {
        session_id: Some(second_session.id.clone()),
        path: "child".to_string(),
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_claude_session(&second_session.id)
            .unwrap()
            .work_dir,
        child.to_string_lossy()
    );
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_claude_session(&first_session.id)
            .unwrap()
            .work_dir,
        first.to_string_lossy()
    );
}
