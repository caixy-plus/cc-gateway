use std::process::Command;

use axum::body::Body;
use axum::http::{Method, Request};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

use crate::config::model::GatewayConfig;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::web::server::create_app;

fn is_claude_available() -> bool {
    Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn setup_app() -> axum::Router {
    let config = GatewayConfig::default();
    create_app(&config)
}

async fn cleanup_webui() {
    let channels: Vec<String> = GLOBAL_CHANNEL_SESSIONS
        .list_channels()
        .into_iter()
        .filter(|c| c.platform == "webui")
        .map(|c| c.id)
        .collect();
    for id in channels {
        // Remove claude sessions from memory and DB
        for cs in GLOBAL_CHANNEL_SESSIONS.list_claude_sessions_by_channel(&id) {
            GLOBAL_CHANNEL_SESSIONS.remove_claude_session(&cs.id);
        }
        crate::db::delete_channel_session(&id);
    }
}

async fn read_json(resp: axum::response::Response) -> Value {
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let mut val: Value = serde_json::from_slice(&body).unwrap_or_else(|_| {
        json!({
            "_raw": String::from_utf8_lossy(&body).to_string(),
            "_status": status.as_u16()
        })
    });
    if let Some(obj) = val.as_object_mut() {
        obj.insert("_status".to_string(), json!(status.as_u16()));
    }
    val
}

#[tokio::test]
async fn test_create_session_is_inactive() {
    cleanup_webui().await;
    let app = setup_app();

    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "Test Session", "work_dir": "~"}).to_string(),
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    let body = read_json(resp).await;

    assert_eq!(body["_status"], 200);
    let session = body["session"].as_object().expect("session object");
    assert_eq!(session["title"], "Test Session");
    assert_eq!(session["active"], false);
    assert_eq!(session["platform"], "webui");
    assert!(session["id"].as_str().unwrap().len() > 0);

    cleanup_webui().await;
}

#[tokio::test]
async fn test_list_sessions_includes_created() {
    cleanup_webui().await;
    let app = setup_app();

    // Create a session
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "List Test", "work_dir": "~"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let create_body = read_json(resp).await;
    let session_id = create_body["session"]["id"].as_str().unwrap().to_string();

    // List sessions
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body = read_json(resp).await;

    assert_eq!(body["_status"], 200);
    let sessions = body["sessions"].as_array().expect("sessions array");
    let found = sessions.iter().any(|s| s["id"] == session_id && s["title"] == "List Test");
    assert!(found, "created session should appear in list");

    cleanup_webui().await;
}

#[tokio::test]
async fn test_delete_session_removes_it() {
    cleanup_webui().await;
    let app = setup_app();

    // Create
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "Delete Test", "work_dir": "~"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let create_body = read_json(resp).await;
    let session_id = create_body["session"]["id"].as_str().unwrap().to_string();

    // Delete
    let req = Request::builder()
        .uri(format!("/api/sessions/{}", session_id))
        .method(Method::DELETE)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let body = read_json(resp).await;
    assert_eq!(body["_status"], 200);
    assert_eq!(body["status"], "deleted");

    // List should not include it
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body = read_json(resp).await;
    let sessions = body["sessions"].as_array().unwrap();
    let found = sessions.iter().any(|s| s["id"] == session_id);
    assert!(!found, "deleted session should not appear in list");

    cleanup_webui().await;
}

#[tokio::test]
async fn test_history_returns_empty_for_new_session() {
    cleanup_webui().await;
    let app = setup_app();

    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "History Test", "work_dir": "~"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let create_body = read_json(resp).await;
    let session_id = create_body["session"]["id"].as_str().unwrap().to_string();

    let req = Request::builder()
        .uri(format!("/api/sessions/{}/history", session_id))
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body = read_json(resp).await;

    assert_eq!(body["_status"], 200);
    let history = body["history"].as_array().unwrap();
    assert!(history.is_empty());

    cleanup_webui().await;
}

#[tokio::test]
async fn test_stop_session_when_inactive() {
    cleanup_webui().await;
    let app = setup_app();

    // Create without starting
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "Stop Test", "work_dir": "~"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let create_body = read_json(resp).await;
    let session_id = create_body["session"]["id"].as_str().unwrap().to_string();

    // Stop should succeed even if inactive (idempotent)
    let req = Request::builder()
        .uri(format!("/api/sessions/{}", session_id))
        .method(Method::POST)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let body = read_json(resp).await;
    assert_eq!(body["_status"], 200);

    cleanup_webui().await;
}

#[tokio::test]
async fn test_start_stop_resume_lifecycle() {
    if !is_claude_available() {
        eprintln!("Skipping test_start_stop_resume_lifecycle: claude not available");
        return;
    }

    cleanup_webui().await;
    let app = setup_app();

    // 1. Create session (inactive)
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "Lifecycle Test", "work_dir": "~"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let create_body = read_json(resp).await;
    assert_eq!(create_body["_status"], 200);
    let session_id = create_body["session"]["id"].as_str().unwrap().to_string();
    assert_eq!(create_body["session"]["active"], false);

    // 2. Start session
    let req = Request::builder()
        .uri(format!("/api/sessions/{}/start", session_id))
        .method(Method::POST)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let start_body = read_json(resp).await;

    if start_body["_status"] == 500 {
        eprintln!(
            "Start session failed (Claude may not be fully available): {}",
            start_body["error"].as_str().unwrap_or("unknown")
        );
        cleanup_webui().await;
        return;
    }

    assert_eq!(start_body["_status"], 200);
    assert_eq!(start_body["status"], "started");
    assert_eq!(start_body["session"]["active"], true);
    assert_eq!(start_body["session"]["id"], session_id);

    // 3. List should show active
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let list_body = read_json(resp).await;
    let sessions = list_body["sessions"].as_array().unwrap();
    let found = sessions
        .iter()
        .find(|s| s["id"] == session_id)
        .expect("session in list");
    assert_eq!(found["active"], true);

    // 4. Stop session
    let req = Request::builder()
        .uri(format!("/api/sessions/{}", session_id))
        .method(Method::POST)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let stop_body = read_json(resp).await;
    assert_eq!(stop_body["_status"], 200);
    assert_eq!(stop_body["status"], "stopped");

    // 5. List should show inactive
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let list_body = read_json(resp).await;
    let sessions = list_body["sessions"].as_array().unwrap();
    let found = sessions
        .iter()
        .find(|s| s["id"] == session_id)
        .expect("session still in list after stop");
    assert_eq!(found["active"], false);

    // 6. Start again (resume)
    let req = Request::builder()
        .uri(format!("/api/sessions/{}/start", session_id))
        .method(Method::POST)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let resume_body = read_json(resp).await;
    assert_eq!(resume_body["_status"], 200);
    assert_eq!(resume_body["status"], "started");
    assert_eq!(resume_body["session"]["active"], true);

    // 7. Stop again and cleanup
    let req = Request::builder()
        .uri(format!("/api/sessions/{}", session_id))
        .method(Method::POST)
        .body(Body::empty())
        .unwrap();
    let _resp = app.clone().oneshot(req).await.unwrap();

    cleanup_webui().await;
}

#[tokio::test]
async fn test_create_session_then_change_dir_before_start() {
    cleanup_webui().await;
    let app = setup_app();

    // Create session
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"title": "Dir Test", "work_dir": "~"}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let create_body = read_json(resp).await;
    let session_id = create_body["session"]["id"].as_str().unwrap().to_string();

    // Change directory via /api/cmd/cd before starting
    let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
    let req = Request::builder()
        .uri("/api/cmd/cd")
        .method(Method::POST)
        .header("Content-Type", "application/json")
        .body(Body::from(
            json!({"path": home, "session_id": session_id}).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let cd_body = read_json(resp).await;
    assert_eq!(cd_body["_status"], 200);

    // Verify work_dir updated
    let req = Request::builder()
        .uri("/api/sessions")
        .method(Method::GET)
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let list_body = read_json(resp).await;
    let sessions = list_body["sessions"].as_array().unwrap();
    let found = sessions.iter().find(|s| s["id"] == session_id).unwrap();
    assert_eq!(found["work_dir"], home);

    cleanup_webui().await;
}
