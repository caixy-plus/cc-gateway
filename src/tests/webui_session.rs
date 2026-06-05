use anyhow::{Context, Result};
use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::db;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::channel_model::AgentSessionState;
use crate::web::handlers::cmd::{handle_cd, CdRequest};
use crate::web::handlers::session::{
    handle_create_session, handle_delete_session, handle_get_history, handle_list_sessions,
    handle_permission, handle_send_message, handle_start_session, handle_stop_session,
    handle_upload_file, AppState, CreateSessionRequest, ListSessionsQuery, PermissionRequest,
    SendMessageRequest, StartSessionRequest,
};
use crate::web::files::WEBUI_FILE_EVENT_PREFIX;
use crate::web::state::EVENT_BUS;

use super::helpers::{ensure_gateway_history, TestEnv};

async fn short_timeout<T>(label: &str, future: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(std::time::Duration::from_secs(20), future)
        .await
        .unwrap_or_else(|_| panic!("webui handler should not hang: {label}"))
}

async fn webui_send(
    state: &AppState,
    session_id: &str,
    message: &str,
) -> (StatusCode, serde_json::Value) {
    let (status, body) = short_timeout(
        "send",
        handle_send_message(
            State(state.clone()),
            Path(session_id.to_string()),
            Json(SendMessageRequest {
                message: message.to_string(),
            }),
        ),
    )
    .await;
    let json = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("webui send should return JSON, status={status}, body={body}"));
    (status, json)
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
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "WebUI flow".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active.clone());
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&session_id)
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
        .get_agent_session(&session_id)
        .expect("session should remain persisted after stop");
    assert!(!stored.active);
    assert_eq!(stored.state, AgentSessionState::Stopped);
    assert!(
        stored.stopped_at.is_some(),
        "stopped_at should be set so WebUI can distinguish resume from first start after refresh"
    );

    let listed = handle_list_sessions(Query(ListSessionsQuery {
        platform: Some("webui".to_string()),
        source: None,
        channel_id: None,
    }))
    .await
    .0;
    let row = listed["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == session_id)
        .expect("session in list");
    assert_eq!(row["active"], false);
    assert!(row["stopped_at"].as_str().is_some());

    Ok(())
}

/// WebUI: start → stop without sending a message → start again (regression: must not pass `--resume`).
#[tokio::test]
async fn webui_start_stop_start_without_chat_succeeds() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-no-chat-restart");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let (status, body) = short_timeout(
        "create",
        handle_create_session(
            State(state.clone()),
            Json(CreateSessionRequest {
                title: Some("No chat restart".to_string()),
                work_dir: Some(work_dir.to_string_lossy().to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = serde_json::from_str::<serde_json::Value>(&body)?["session"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, body) = short_timeout(
        "start1",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest {
                provider: Some("claude".to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "started"
    );
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .unwrap()
        .provider_session_id
        .is_some());

    let (status, _) = short_timeout("stop", handle_stop_session(Path(session_id.clone()))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&session_id)
            .unwrap()
            .active
    );

    let (status, body) = short_timeout(
        "start2",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest::default()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "started"
    );
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&session_id)
            .unwrap()
            .active
    );

    Ok(())
}

/// After real chat history exists, resume must pass `--resume` with the stored provider session id.
#[tokio::test]
async fn webui_resume_after_chat_passes_claude_resume_flag() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-resume-memory");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let (status, body) = short_timeout(
        "create",
        handle_create_session(
            State(state.clone()),
            Json(CreateSessionRequest {
                title: Some("Resume memory".to_string()),
                work_dir: Some(work_dir.to_string_lossy().to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = serde_json::from_str::<serde_json::Value>(&body)?["session"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = short_timeout(
        "start1",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest {
                provider: Some("claude".to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let provider_id = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .unwrap()
        .provider_session_id
        .clone()
        .expect("provider_session_id after first start");
    assert_eq!(
        std::fs::read_to_string(env.home().join(".cc-gateway/.test_last_resume"))?,
        "none"
    );

    let mut rx = EVENT_BUS.subscribe();
    let (status, _) = short_timeout(
        "send",
        handle_send_message(
            State(state.clone()),
            Path(session_id.clone()),
            Json(SendMessageRequest {
                message: "remember this".to_string(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_webui_events(&mut rx, &session_id, "remember this").await?;

    let history_path = env
        .home()
        .join(".cc-gateway/history")
        .join(format!("{}.jsonl", session_id));
    ensure_gateway_history(&history_path).await?;

    let (status, _) = short_timeout("stop", handle_stop_session(Path(session_id.clone()))).await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = short_timeout(
        "start2",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest::default()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(env.home().join(".cc-gateway/.test_last_resume"))?,
        provider_id,
        "second start should resume Claude with the persisted provider session id"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_send_message_ensures_poller_for_existing_active_runtime() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-active-no-poller");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "Active without poller".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

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
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "No empty assistant event".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_history_records_user_message_once_after_send() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-history-dedupe");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "History dedupe".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

    let user_text = "你好";
    let (status, _) = webui_send(&state, &session_id, user_text).await;
    assert_eq!(status, StatusCode::OK);

    let history_path = env
        .home()
        .join(".cc-gateway/history")
        .join(format!("{session_id}.jsonl"));
    ensure_gateway_history(&history_path).await?;

    let (status, body) = handle_get_history(Path(session_id.clone())).await;
    assert_eq!(status, StatusCode::OK);
    let history = serde_json::from_str::<serde_json::Value>(&body)?["history"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let user_lines: Vec<_> = history
        .iter()
        .filter(|line| line["role"] == "user" && line["content"] == user_text)
        .collect();
    assert_eq!(
        user_lines.len(),
        1,
        "user message should appear once in history after refresh, got {user_lines:?}"
    );

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
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
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
        platform: Some("webui".to_string()),
        source: None,
        channel_id: None,
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
        .get_agent_session(&session_id)
        .is_none());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_delete_session_removes_history_jsonl() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-delete-history");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "Delete history file".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

    let (status, _) = webui_send(&state, &session_id, "hello").await;
    assert_eq!(status, StatusCode::OK);

    let history_file = env
        .home()
        .join(".cc-gateway")
        .join("history")
        .join(format!("{session_id}.jsonl"));
    ensure_gateway_history(&history_file).await?;

    let _ = short_timeout("stop", handle_stop_session(Path(session_id.clone()))).await;

    let (status, body) = handle_delete_session(Path(session_id.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "deleted"
    );
    assert!(
        !history_file.exists(),
        "history jsonl should be removed when session is deleted"
    );

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
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: default_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
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
        platform: Some("webui".to_string()),
        source: None,
        channel_id: None,
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
        handle_start_session(
            State(state),
            Path(session_id.clone()),
            Json(StartSessionRequest::default()),
        ),
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
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: default_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
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
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "Active delete protection".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: env.fake_agent_profiles(),
                show_thinking: false,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

    let (status, body) = handle_delete_session(Path(session_id.clone())).await;

    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["error"],
        crate::t!("webui.cannot_delete_active")
    );
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&session_id)
            .unwrap()
            .active
    );
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_webui_runtime(&runtime.channel_session.id)
        .unwrap()
        .active_agents
        .get(&session_id)
        .is_some());

    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_permission_allow_and_deny_endpoint() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-permission");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "Permission test".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&runtime.channel_session.id, active);

    // Allow
    let (status, body) = short_timeout(
        "permission allow",
        handle_permission(
            State(state.clone()),
            Path(session_id.clone()),
            Json(PermissionRequest {
                request_id: "req-1".to_string(),
                action: "allow".to_string(),
                reason: None,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["action"], "allow");

    // Deny with reason
    let (status, body) = short_timeout(
        "permission deny",
        handle_permission(
            State(state.clone()),
            Path(session_id.clone()),
            Json(PermissionRequest {
                request_id: "req-2".to_string(),
                action: "deny".to_string(),
                reason: Some("Not now".to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let body: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["request_id"], "req-2");
    assert_eq!(body["action"], "deny");

    // Invalid action
    let (status, _body) = short_timeout(
        "permission invalid",
        handle_permission(
            State(state.clone()),
            Path(session_id.clone()),
            Json(PermissionRequest {
                request_id: "req-3".to_string(),
                action: "bogus".to_string(),
                reason: None,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Missing session
    let (status, _body) = short_timeout(
        "permission missing session",
        handle_permission(
            State(state.clone()),
            Path("nonexistent-id".to_string()),
            Json(PermissionRequest {
                request_id: "req-4".to_string(),
                action: "allow".to_string(),
                reason: None,
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;
    Ok(())
}

/// WebUI `POST /api/sessions/:id/start` must attach cc-gateway MCP (WebUi `send_file` target).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_handle_start_injects_mcp_context() -> Result<()> {
    let env = TestEnv::new();
    super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;
    let work_dir = env.home().join("webui-mcp-start");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let (status, body) = short_timeout(
        "create",
        handle_create_session(
            State(state.clone()),
            Json(CreateSessionRequest {
                title: Some("MCP inject".to_string()),
                work_dir: Some(work_dir.to_string_lossy().to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = serde_json::from_str::<serde_json::Value>(&body)?["session"]["id"]
        .as_str()
        .context("session id")?
        .to_string();

    let (status, _) = short_timeout(
        "start",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest {
                provider: Some("claude".to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mcp_marker = env.home().join(".cc-gateway/.test_last_mcp_config");
    assert!(
        mcp_marker.is_file(),
        "WebUI start should pass --mcp-config when mcp_context is set"
    );
    let path = std::fs::read_to_string(&mcp_marker)?;
    let config_body = std::fs::read_to_string(path.trim())?;
    assert!(
        config_body.contains("cc-gateway") && config_body.contains("_mcp-server"),
        "mcp config should reference cc-gateway MCP server"
    );
    assert!(
        config_body.contains("web_ui") && config_body.contains(&session_id),
        "WebUI MCP target should include platform web_ui and session id, got: {config_body}"
    );

    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;
    Ok(())
}

/// Same scenario as `smoke_core::core_claude_session_flow_in_test_work_dir`, via WebUI HTTP handlers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_core_claude_session_flow_in_test_work_dir() -> Result<()> {
    let env = TestEnv::new_with_repo_work_dir();
    super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;

    let root = env.repo_work_dir();
    assert!(
        root.is_dir(),
        "test_work_dir should exist at {}",
        root.display()
    );

    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: root.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let memory_token = format!("MEM-{}", Uuid::new_v4());
    let quick_prompt = format!("reply ok {memory_token}");

    let (status, body) = short_timeout(
        "create",
        handle_create_session(
            State(state.clone()),
            Json(CreateSessionRequest {
                title: Some("WebUI core flow".to_string()),
                work_dir: Some(root.to_string_lossy().to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id = serde_json::from_str::<serde_json::Value>(&body)?["session"]["id"]
        .as_str()
        .context("session id")?
        .to_string();

    let (status, _) = short_timeout(
        "start",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest {
                provider: Some("claude".to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let provider_id = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .unwrap()
        .provider_session_id
        .clone()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "fake-session".to_string());

    let channel_id = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .unwrap()
        .channel_session_id
        .clone();
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_webui_active_agent(&channel_id, &session_id)
            .is_some(),
        "WebUI runtime should track the started session"
    );

    let mut rx = EVENT_BUS.subscribe();
    let (status, _) = short_timeout(
        "chat",
        handle_send_message(
            State(state.clone()),
            Path(session_id.clone()),
            Json(SendMessageRequest {
                message: quick_prompt.clone(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_webui_events(&mut rx, &session_id, &quick_prompt).await?;

    let history_path = env
        .home()
        .join(".cc-gateway/history")
        .join(format!("{session_id}.jsonl"));
    ensure_gateway_history(&history_path).await?;

    // `/stop` — subprocess stays up (WebUI chat command, not sidebar stop)
    let (status, body) = webui_send(&state, &session_id, "/stop").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("response").is_some());
    let active = GLOBAL_CHANNEL_SESSIONS
        .get_webui_active_agent(&channel_id, &session_id)
        .expect("session should stay in WebUI runtime after /stop");
    assert!(
        active.controller.lock().await.is_session_active().await,
        "agent subprocess should remain after /stop"
    );

    // Sidebar/history APIs (chat `/agent-history` is only for inactive sessions in the router)
    let listed = handle_list_sessions(Query(ListSessionsQuery {
        platform: Some("webui".to_string()),
        source: None,
        channel_id: None,
    }))
    .await
    .0;
    assert!(
        listed["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"] == session_id),
        "WebUI session list should include the active session"
    );

    // `/quit` — tears down subprocess (WebUI stop kind)
    let (status, body) = webui_send(&state, &session_id, "/quit").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"].as_str(), Some("stopped"));
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_webui_active_agent(&channel_id, &session_id)
            .is_none(),
        "WebUI runtime should drop active agent after /quit"
    );
    let stored = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .unwrap();
    assert!(!stored.active);
    assert!(stored.stopped_at.is_some());

    std::fs::write(
        env.home().join(".cc-gateway/.test_agent_session_id"),
        &session_id,
    )?;

    let (status, _) = short_timeout(
        "resume",
        handle_start_session(
            State(state.clone()),
            Path(session_id.clone()),
            Json(StartSessionRequest::default()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(env.home().join(".cc-gateway/.test_last_resume"))?,
        provider_id,
        "WebUI resume should pass stored provider session id to claude"
    );
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_webui_active_agent(&channel_id, &session_id)
        .is_some());

    let mut rx = EVENT_BUS.subscribe();
    let (status, _) = short_timeout(
        "ping",
        handle_send_message(
            State(state.clone()),
            Path(session_id.clone()),
            Json(SendMessageRequest {
                message: "ping".to_string(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_webui_events(&mut rx, &session_id, "ping").await?;
    let hist_after = std::fs::read_to_string(&history_path)?;
    assert!(
        hist_after.contains(&memory_token),
        "gateway history must retain the prior user turn after WebUI resume"
    );

    let (status, _) = webui_send(&state, &session_id, "/quit").await;
    assert_eq!(status, StatusCode::OK);

    // New tab session: `/stop` → `/clear` → `/quit`
    let (status, body) = short_timeout(
        "create2",
        handle_create_session(
            State(state.clone()),
            Json(CreateSessionRequest {
                title: Some("WebUI core flow 2".to_string()),
                work_dir: Some(root.to_string_lossy().to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session_id_2 = serde_json::from_str::<serde_json::Value>(&body)?["session"]["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = short_timeout(
        "start2",
        handle_start_session(
            State(state.clone()),
            Path(session_id_2.clone()),
            Json(StartSessionRequest {
                provider: Some("claude".to_string()),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let mut rx = EVENT_BUS.subscribe();
    let (status, _) = short_timeout(
        "warmup",
        handle_send_message(
            State(state.clone()),
            Path(session_id_2.clone()),
            Json(SendMessageRequest {
                message: "warmup".to_string(),
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_webui_events(&mut rx, &session_id_2, "warmup").await?;

    let (status, _) = webui_send(&state, &session_id_2, "/stop").await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = webui_send(&state, &session_id_2, "/clear").await;
    assert_eq!(status, StatusCode::OK);
    let active2 = GLOBAL_CHANNEL_SESSIONS
        .get_webui_active_agent(&channel_id, &session_id_2)
        .expect("session 2 still active after /clear");
    assert!(
        active2.controller.lock().await.is_session_active().await,
        "subprocess should remain after /clear"
    );
    let (status, _) = webui_send(&state, &session_id_2, "/quit").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = handle_delete_session(Path(session_id.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&body)?["status"],
        "deleted"
    );
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .is_none());
    assert!(GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id_2)
        .is_some());

    Ok(())
}

/// WebUI file upload should render the attachment card once and not duplicate the agent
/// prompt as a second user chat bubble (forward uses `echo_user_to_ui = false`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn webui_upload_forward_skips_duplicate_user_bubble() -> Result<()> {
    use axum::body::Body;
    use axum::extract::{FromRequest, Multipart};
    use axum::http::Request;

    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("webui-upload-dedupe");
    std::fs::create_dir_all(&work_dir)?;
    let state = AppState {
        agent_settings: env.fake_agent_profiles(),
        show_thinking: false,
        default_dir: work_dir.to_string_lossy().to_string(),
        daemon_config_path: None,
        allowed_ips: vec![],
        webui_token: None,
    };

    let runtime = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_webui_channel("WebUI", work_dir.to_str().unwrap())
        .await?;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: runtime.channel_session.id.clone(),
                title: "Upload dedupe".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: state.agent_settings.clone(),
                show_thinking: state.show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    let channel_id = runtime.channel_session.id.clone();
    GLOBAL_CHANNEL_SESSIONS.set_webui_active_agent(&channel_id, active);

    let mut rx = EVENT_BUS.subscribe();
    while rx.try_recv().is_ok() {}

    let boundary = "----ccgtestboundary";
    let file_body = "# Title\n\nupload body";
    let multipart_body = format!(
        "--{boundary}\r\n\
         Content-Disposition: form-data; name=\"file\"; filename=\"note.md\"\r\n\
         Content-Type: text/markdown\r\n\
         \r\n\
         {file_body}\r\n\
         --{boundary}--\r\n"
    );
    let req = Request::builder()
        .header(
            axum::http::header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(multipart_body))
        .unwrap();
    let multipart = Multipart::from_request(req, &())
        .await
        .map_err(|e| anyhow::anyhow!("multipart parse failed: {e}"))?;

    let (status, body) = short_timeout(
        "upload",
        handle_upload_file(
            State(state.clone()),
            Path(session_id.clone()),
            multipart,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "upload response: {body}");
    let parsed = serde_json::from_str::<serde_json::Value>(&body)?;
    assert_eq!(parsed["forwarded"], true);

    let mut file_cards = 0u32;
    let mut inlined_prompt_bubbles = 0u32;
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await {
            Ok(Ok(event)) if event.session_id == session_id => {
                if event.role == "user" && event.content.starts_with(WEBUI_FILE_EVENT_PREFIX) {
                    file_cards += 1;
                }
                if event.role == "user" && event.content.contains("# Title") {
                    inlined_prompt_bubbles += 1;
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
            Err(_) => break,
        }
        if file_cards >= 1 {
            break;
        }
    }

    assert_eq!(file_cards, 1, "expected one file attachment card event");
    assert_eq!(
        inlined_prompt_bubbles, 0,
        "inlined agent prompt must not appear as a duplicate user bubble"
    );

    let _ = short_timeout("stop", handle_stop_session(Path(session_id))).await;
    Ok(())
}
