//! ACP `session/load` resume behavior against a fake ACP CLI.
//!
//! Per the ACP spec, `session/load` replays the prior conversation via
//! `session/update` notifications before the load response. The gateway renders
//! history from its own JSONL, so replayed chunks must NOT be re-emitted as live
//! agent output — and a failed `session/load` must fall back to `session/new`.
#![cfg(not(windows))]

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;

use super::helpers::TestEnv;
use crate::agent::codex_acp::CodexAcpSession;
use crate::agent::event::AgentEvent;
use crate::config::model::{AgentConfig, AgentProvider};

/// Fake ACP server: NDJSON JSON-RPC over stdio.
/// With argv[1] == "fail-load", `session/load` returns a JSON-RPC error.
/// Otherwise `session/load` replays two history chunks before its response.
fn create_fake_acp_cli(home: &Path) -> PathBuf {
    let script = home.join("fake-acp");
    std::fs::write(
        &script,
        r#"#!/bin/sh
mode="$1"
while IFS= read -r line || [ -n "$line" ]; do
  [ -z "$line" ] && continue
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentCapabilities":{"loadSession":true}}}\n' "$id"
      ;;
    *'"method":"session/load"'*)
      if [ "$mode" = "fail-load" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"Method not found"}}\n' "$id"
      else
        sid=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"user_message_chunk","content":{"type":"text","text":"replayed-user"}}}}\n' "$sid"
        printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"replayed-reply"}}}}\n' "$sid"
        printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"%s"}}\n' "$id" "$sid"
      fi
      ;;
    *'"method":"session/new"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"fresh-session"}}\n' "$id"
      ;;
    *'"method":"session/prompt"'*)
      sid=$(printf '%s' "$line" | sed -n 's/.*"sessionId":"\([^"]*\)".*/\1/p')
      printf '{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"%s","update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"live-reply"}}}}\n' "$sid"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id"
      ;;
    *)
      if [ -n "$id" ]; then
        printf '{"jsonrpc":"2.0","id":%s,"result":{}}\n' "$id"
      fi
      ;;
  esac
done
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn fake_acp_config(script: &Path) -> AgentConfig {
    let mut config = AgentConfig::default_for_provider(AgentProvider::Codex);
    config.cli_path = script.display().to_string();
    config
}

fn make_work_dir(env: &TestEnv) -> String {
    let dir = env.home().join("work");
    std::fs::create_dir_all(&dir).unwrap();
    dir.display().to_string()
}

async fn recv_with_timeout(
    rx: &mut mpsc::UnboundedReceiver<AgentEvent>,
) -> Option<AgentEvent> {
    tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .ok()
        .flatten()
}

#[tokio::test]
async fn acp_resume_suppresses_history_replay_during_session_load() {
    let env = TestEnv::new();
    let script = create_fake_acp_cli(env.home());
    let config = fake_acp_config(&script);
    let work_dir = make_work_dir(&env);
    let (tx, mut rx) = mpsc::unbounded_channel();

    let (session, spawned_id) = CodexAcpSession::spawn(
        work_dir,
        vec![],
        &config,
        tx,
        Some("sess-resume-1".to_string()),
        None,
    )
    .await
    .expect("resume spawn should succeed");
    assert_eq!(spawned_id.as_deref(), Some("sess-resume-1"));

    // Replayed history chunks must NOT leak as live output. The only expected
    // pre-prompt event is SessionId.
    let mut saw_session_id = false;
    while let Ok(ev) = rx.try_recv() {
        match ev {
            AgentEvent::SessionId(id) => {
                assert_eq!(id, "sess-resume-1");
                saw_session_id = true;
            }
            AgentEvent::Text(t) | AgentEvent::Thinking(t) => {
                panic!("replayed history leaked as live output: {t:?}")
            }
            AgentEvent::Done => panic!("replay must not emit Done before any prompt"),
            _ => {}
        }
    }
    assert!(saw_session_id, "SessionId event should be emitted on spawn");

    // Live prompts after resume must still stream (guards against the replay
    // gate being stuck on).
    session.send_user_message("hi").await.expect("prompt send");
    let mut live_text = None;
    while let Some(ev) = recv_with_timeout(&mut rx).await {
        match ev {
            AgentEvent::Text(t) => live_text = Some(t),
            AgentEvent::Done => break,
            AgentEvent::Error(e) => panic!("prompt errored: {e}"),
            _ => {}
        }
    }
    assert_eq!(live_text.as_deref(), Some("live-reply"));

    session.stop().await.ok();
}

#[tokio::test]
async fn acp_resume_falls_back_to_session_new_when_load_fails() {
    let env = TestEnv::new();
    let script = create_fake_acp_cli(env.home());
    let config = fake_acp_config(&script);
    let work_dir = make_work_dir(&env);
    let (tx, mut rx) = mpsc::unbounded_channel();

    // extra_args[0] becomes argv[1] of the fake script → fail session/load.
    let (session, spawned_id) = CodexAcpSession::spawn(
        work_dir,
        vec!["fail-load".to_string()],
        &config,
        tx,
        Some("sess-expired".to_string()),
        None,
    )
    .await
    .expect("spawn should fall back to session/new instead of failing");
    assert_eq!(spawned_id.as_deref(), Some("fresh-session"));

    let mut saw_session_id = false;
    while let Ok(ev) = rx.try_recv() {
        if let AgentEvent::SessionId(id) = ev {
            assert_eq!(id, "fresh-session");
            saw_session_id = true;
        }
    }
    assert!(saw_session_id, "fallback session id should be emitted");

    session.stop().await.ok();
}
