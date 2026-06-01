use std::path::PathBuf;

use axum::extract::Json;
use axum::http::StatusCode;

use crate::db;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::handlers::cmd::{handle_cd, handle_ll, handle_pwd, CdRequest, LlRequest, SessionCmdRequest};

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
async fn webui_cd_updates_active_agent_session_work_dir() {
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
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "WebUI cd active".to_string(),
                default_dir: root.to_string_lossy().to_string(),
                agent_settings: env.fake_agent_profiles(),
                show_thinking: false,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await
        .unwrap();
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

    let (status, body) = handle_cd(Json(CdRequest {
        session_id: Some(session_id.clone()),
        path: child.to_string_lossy().to_string(),
    }))
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&session_id)
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
        .create_agent_session_only(
            &runtime.channel_session.id,
            "First inactive",
            first.to_str().unwrap(),
            "claude",
        )
        .unwrap();
    let second_session = GLOBAL_CHANNEL_SESSIONS
        .create_agent_session_only(
            &runtime.channel_session.id,
            "Second inactive",
            second.to_str().unwrap(),
            "claude",
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
            .get_agent_session(&second_session.id)
            .unwrap()
            .work_dir,
        child.to_string_lossy()
    );
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&first_session.id)
            .unwrap()
            .work_dir,
        first.to_string_lossy()
    );
}

#[tokio::test]
async fn webui_ll_lists_nested_dir_via_absolute_path_without_changing_work_dir() {
    let env = TestEnv::new();
    db::init_schema().unwrap();
    let root = env.home().join("webui-ll-browse-root");
    let child = root.join("child");
    let nested = child.join("nested");
    std::fs::create_dir_all(&nested).unwrap();

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", root.to_str().unwrap())
        .await
        .unwrap();
    let session = GLOBAL_CHANNEL_SESSIONS
        .create_agent_session_only(
            &runtime.channel_session.id,
            "LL browse",
            root.to_str().unwrap(),
            "claude",
        )
        .unwrap();

    let (status, body) = handle_ll(Json(LlRequest {
        session_id: Some(session.id.clone()),
        path: Some(nested.to_string_lossy().to_string()),
        show_hidden: None,
    }))
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    let listed = PathBuf::from(json["dir"].as_str().unwrap());
    assert_eq!(
        listed.canonicalize().unwrap(),
        nested.canonicalize().unwrap()
    );
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&session.id)
            .unwrap()
            .work_dir,
        root.to_string_lossy()
    );
}
