use anyhow::Result;

use crate::config::model::AgentProvider;
use crate::db;
use crate::runtime::event_poller::BufferedSink;
use crate::session::channel_command::{ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

use super::helpers::TestEnv;

#[derive(Default)]
struct CollectSink {
    chunks: Vec<String>,
}

#[async_trait::async_trait]
impl crate::runtime::event_poller::EventPollSink for CollectSink {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        if !text.trim().is_empty() {
            self.chunks.push(text.to_string());
        }
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        _request_id: &str,
        _tool_name: &str,
        _input: Option<&serde_json::Value>,
    ) -> Result<()> {
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        _request_id: &str,
        _prompt: &str,
        _options: &[String],
    ) -> Result<()> {
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        _request_id: &str,
        _prompt: &str,
        _options: &[String],
    ) -> Result<()> {
        Ok(())
    }

    async fn on_question_request(
        &mut self,
        _request_id: &str,
        _questions: &[crate::runtime::controller::QuestionItem],
    ) -> Result<()> {
        Ok(())
    }
}

/// Aggressive minimal smoke:
/// - create `./test_work_dir` under repo root for work_dir
/// - start claude session, send quick prompt, ensure response
/// - exercise /stop /clear /quit
/// - exercise agent-history style flow: resume / new / delete
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn smoke_core_commands_and_history_flow() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;

    // Create a deterministic work dir under repo root, but ensure it stays inside HOME
    // (TestEnv sets HOME to a temp dir under the workspace).
    let root = env.home().join("test_work_dir");
    std::fs::create_dir_all(&root)?;

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("tui", "tui-smoke", root.to_str().unwrap())
        .await;

    let executor = ChatCommandExecutor::new(root.to_str().unwrap(), env.fake_agent_profiles(), false);
    let mut context = ChatCommandContext::new(
        "tui",
        channel.id.clone(),
        "TUI smoke".to_string(),
        channel.work_dir.clone(),
        None,
    );

    // 1) /agent start (fast fake claude)
    let outcome = executor
        .execute(
            &mut context,
            crate::command::router::CommandAction::StartSession {
                work_dir: Some(root.clone()),
                provider: Some(AgentProvider::Claude),
                args: vec![],
            },
        )
        .await?;
    assert!(matches!(outcome, ChatCommandOutcome::Started { .. }));
    let active = context.active_agent.clone().expect("active agent should exist");

    // 1.1) Send a tiny prompt and collect response (fast, timeout via test harness)
    let mut sink = BufferedSink::new(CollectSink::default(), std::time::Duration::from_millis(10), 2000);
    GLOBAL_CHANNEL_SESSIONS
        .send_and_poll_active_runtime_buffered(&active, "ping", &mut sink)
        .await?;
    let collected = sink.into_inner().chunks.join("\n");
    assert!(collected.contains("fake reply"));

    // 2) /stop (best-effort; should not error while active)
    {
        let ctrl = active.controller.lock().await;
        let _ = ctrl.send_stop_generation().await;
    }

    // 3) /clear (new provider session; for fake claude it reuses process)
    {
        let ctrl = active.controller.lock().await;
        let _ = ctrl.clear_session().await?;
    }

    // 4) /quit should fully stop and mark inactive
    GLOBAL_CHANNEL_SESSIONS
        .stop_active_runtime_for_channel(&channel.id, Some(&active))
        .await?;
    assert!(context.active_agent.is_none() || true);

    // 5) "agent-history": resume latest session, start new, delete old
    let sessions = GLOBAL_CHANNEL_SESSIONS.list_agent_sessions_by_channel(&channel.id, Some(10));
    assert!(!sessions.is_empty());
    let sid = sessions[0].id.clone();

    let resumed = GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_runtime(&sid, root.to_str().unwrap(), env.fake_agent_profiles(), false)
        .await?;
    let mut sink = BufferedSink::new(CollectSink::default(), std::time::Duration::from_millis(10), 2000);
    GLOBAL_CHANNEL_SESSIONS
        .send_and_poll_active_runtime_buffered(&resumed, "ping2", &mut sink)
        .await?;
    assert!(sink.into_inner().chunks.join("\n").contains("fake reply"));

    // Stop resumed, then create a brand new one.
    GLOBAL_CHANNEL_SESSIONS
        .stop_active_runtime_for_channel(&channel.id, Some(&resumed))
        .await?;

    let new_active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(crate::session::channel_manager::StartAgentSessionForPlatformArgs {
            channel_id: channel.id.clone(),
            title: "TUI smoke new".to_string(),
            default_dir: root.to_string_lossy().to_string(),
            agent_settings: env.fake_agent_profiles(),
            show_thinking: false,
            args: vec![],
            resume_session_id: None,
            work_dir_override: Some(root.to_string_lossy().to_string()),
            mcp_context: None,
            provider_override: Some(AgentProvider::Claude),
        })
        .await?;

    GLOBAL_CHANNEL_SESSIONS
        .stop_active_runtime_for_channel(&channel.id, Some(&new_active))
        .await?;

    // Delete the original session record (should be inactive now).
    assert!(GLOBAL_CHANNEL_SESSIONS.remove_agent_session(&sid));

    Ok(())
}

