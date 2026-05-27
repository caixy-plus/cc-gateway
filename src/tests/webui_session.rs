use anyhow::Result;
use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;

use crate::db;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::channel_model::ClaudeSessionState;
use crate::web::handlers::cmd::{handle_cd, CdRequest};
use crate::web::handlers::session::{
    handle_create_session, handle_delete_session, handle_get_history, handle_list_sessions,
    handle_send_message, handle_start_session, handle_stop_session, AppState, CreateSessionRequest,
    ListSessionsQuery, SendMessageRequest,
};
use crate::web::state::EVENT_BUS;

use super::helpers::TestEnv;

async fn short_timeout<T>(label: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(20), future)
        .await
        .unwrap_or_else(|_| panic!("webui handler should not hang: {label}"))
}

async fn assert_webui_events(
    rx: &mut tokio::sync::broadcast::Receiver<crate::web::state::Event>,
    session_id: &str,
    user_text: &str,
) -> Result<()> {
    let mut saw_user = false;
    let mut saw_assistant = false;
    for _ in 0..6 {
        let event = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await??;
        if event.session_id != session_id {
            continue;
        }
        if event.role == "user" && event.content == user_text {
            saw_user = true;
        }
        if event.role == "assistant" && event.content.contains("fake reply") {
            saw_assistant = true;
        }
        if saw_user && saw_assistant {
            break;
        }
    }

    assert!(saw_user);
    assert!(saw_assistant);
    Ok(())
}

async fn assert_no_empty_assistant_event(
    rx: &mut tokio::sync::broadcast::Receiver<crate::web::state::Event>,
    session_id: &str,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(event)) if event.session_id == session_id => {
                assert!(
                    !(event.role == "assistant" && event.content.trim().is_empty()),
                    "WebUI should not receive empty assistant events"
                );
            }
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
            Err(_) => break,
        }
    }
    Ok(())
}

#[tokio::test]
async fn webui_session_create_start_send_and_stop_updates_events_and_db() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-project");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        claude_config: env.fake_claude_config().into(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_claude_session_for_platform(
            &runtime.channel_session.id,
            "WebUI flow",
            work_dir.to_str().unwrap(),
            state.claude_config.clone(),
            state.show_thinking,
            vec![],
            None,
            None,
            None,
        )
        .await?;
    let session_id = active.claude_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS
        .set_webui_active_claude(&runtime.channel_session.id, Some(active.clone()));
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_claude_session(&session_id)
            .unwrap()
            .active
    );

    let mut rx = EVENT_BUS.subscribe();
    let (status, body) = short_timeout(
        "send",
        handle_send_message(
            State(state.clone()),
            Path(session_id.clone()),
            Json(SendMessageRequest {
                message: "hello".to_string(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "forwarded"
    );

    assert_webui_events(&mut rx, &session_id, "hello").await?;

    let (status, _) = short_timeout("stop", handle_stop_session(Path(session_id.clone()))).await;
    assert_eq!(status, StatusCode::OK);
    let stored = GLOBAL_CHANNEL_SESSIONS
        .get_claude_session(&session_id)
        .expect("session should remain persisted after stop");
    assert!(!stored.active);
    assert_eq!(stored.state, ClaudeSessionState::Stopped);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_send_message_ensures_poller_for_existing_active_runtime() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-active-no-poller");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        claude_config: env.fake_claude_config().into(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_claude_session_for_platform(
            &runtime.channel_session.id,
            "Active without poller",
            work_dir.to_str().unwrap(),
            state.claude_config.clone(),
            state.show_thinking,
            vec![],
            None,
            None,
            None,
        )
        .await?;
    let session_id = active.claude_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&runtime.channel_session.id, Some(active));

    let mut rx = EVENT_BUS.subscribe();
    let (status, body) = short_timeout(
        "send",
        handle_send_message(
            State(state.clone()),
            Path(session_id.clone()),
            Json(SendMessageRequest {
                message: "hello after poller loss".to_string(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "forwarded"
    );

    assert_webui_events(&mut rx, &session_id, "hello after poller loss").await?;
    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_poller_does_not_broadcast_empty_assistant_done_event() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-empty-assistant");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        claude_config: env.fake_claude_config().into(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_claude_session_for_platform(
            &runtime.channel_session.id,
            "No empty assistant event",
            work_dir.to_str().unwrap(),
            state.claude_config.clone(),
            state.show_thinking,
            vec![],
            None,
            None,
            None,
        )
        .await?;
    let session_id = active.claude_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&runtime.channel_session.id, Some(active));

    let mut rx = EVENT_BUS.subscribe();
    let (status, body) = short_timeout(
        "send",
        handle_send_message(
            State(state.clone()),
            Path(session_id.clone()),
            Json(SendMessageRequest {
                message: "hello without empty bubble".to_string(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "forwarded"
    );

    assert_webui_events(&mut rx, &session_id, "hello without empty bubble").await?;
    assert_no_empty_assistant_event(&mut rx, &session_id).await?;
    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;

    Ok(())
}

#[tokio::test]
async fn webui_list_history_and_delete_session_handlers_are_offline_testable() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-list-delete");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        claude_config: env.fake_claude_config().into(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
    };

    let (status, body) = handle_create_session(
        State(state),
        Json(CreateSessionRequest {
            title: Some("List delete flow".to_string()),
            work_dir: Some(work_dir.to_string_lossy().to_string()),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body)?;
    let session_id = body["session"]["id"].as_str().unwrap().to_string();

    let listed = handle_list_sessions(Query(ListSessionsQuery {
        source: Some("webui".to_string()),
    }))
    .await
    .0;
    assert_eq!(listed["sessions"].as_array().unwrap().len(), 1);
    assert_eq!(listed["sessions"][0]["id"], session_id);

    let (status, body) = handle_get_history(Path(session_id.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["history"],
        serde_json::json!([])
    );

    let (status, body) = handle_delete_session(Path(session_id.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "deleted"
    );
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_claude_session(&session_id)
        .is_none());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_created_session_keeps_selected_work_dir_when_listed_and_started() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let default_dir = env.home();
    let selected_dir = env.home().join("Downloads");
    std::fs::create_dir_all(&selected_dir)?;
    let state = AppState {
        claude_config: env.fake_claude_config().into(),
        show_thinking: false,
        default_dir: default_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
    };

    let (status, body) = handle_create_session(
        State(state.clone()),
        Json(CreateSessionRequest {
            title: Some("Selected download dir".to_string()),
            work_dir: Some(default_dir.to_string_lossy().to_string()),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body)?;
    let session_id = body["session"]["id"].as_str().unwrap().to_string();
    assert_eq!(
        body["session"]["work_dir"].as_str().unwrap(),
        default_dir.to_str().unwrap()
    );

    let (status, body) = handle_cd(Json(CdRequest {
        session_id: Some(session_id.clone()),
        path: selected_dir.to_string_lossy().to_string(),
    }))
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["dir"]
            .as_str()
            .unwrap(),
        selected_dir.to_str().unwrap()
    );

    let listed = handle_list_sessions(Query(ListSessionsQuery {
        source: Some("webui".to_string()),
    }))
    .await
    .0;
    let listed_session = listed["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|session| session["id"] == session_id)
        .unwrap();
    assert_eq!(
        listed_session["work_dir"].as_str().unwrap(),
        selected_dir.to_str().unwrap()
    );

    let (status, body) = short_timeout(
        "start selected work dir session",
        handle_start_session(State(state), Path(session_id.clone())),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(
        body["session"]["work_dir"].as_str().unwrap(),
        selected_dir.to_str().unwrap()
    );

    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;
    Ok(())
}

#[tokio::test]
async fn webui_create_session_treats_tilde_as_config_default_dir() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let default_dir = env.home().join("configured-default");
    std::fs::create_dir_all(&default_dir)?;
    let state = AppState {
        claude_config: env.fake_claude_config().into(),
        show_thinking: false,
        default_dir: default_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
    };

    let (status, body) = handle_create_session(
        State(state),
        Json(CreateSessionRequest {
            title: Some("Default dir session".to_string()),
            work_dir: Some("~".to_string()),
        }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{body}");
    let body: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(
        body["session"]["work_dir"].as_str().unwrap(),
        default_dir.to_str().unwrap()
    );
    Ok(())
}

#[tokio::test]
async fn webui_delete_session_rejects_active_session_without_stopping_it() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-active-delete");
    std::fs::create_dir_all(&work_dir)?;

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_claude_session_for_platform(
            &runtime.channel_session.id,
            "Active delete protection",
            work_dir.to_str().unwrap(),
            env.fake_claude_config(),
            false,
            vec![],
            None,
            None,
            None,
        )
        .await?;
    let session_id = active.claude_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_claude(&runtime.channel_session.id, Some(active));

    let (status, body) = handle_delete_session(Path(session_id.clone())).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["error"],
        crate::t!("webui.cannot_delete_active")
    );
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_claude_session(&session_id)
            .unwrap()
            .active
    );
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_webui_runtime(&runtime.channel_session.id)
        .unwrap()
        .active_claude
        .is_some());

    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;

    Ok(())
}
