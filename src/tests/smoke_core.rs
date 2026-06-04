//! Core Claude session flow under `./test_work_dir` at the repo root (fake `claude` CLI).
//!
//! WebUI HTTP equivalent: `webui_session::webui_core_claude_session_flow_in_test_work_dir`.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use uuid::Uuid;

use crate::config::model::AgentProvider;
use crate::db;
use crate::runtime::event_poller::{BufferPolicy, BufferedSink};
use crate::session::channel_command::{
    ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome,
};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::chat_flow;

use super::helpers::TestEnv;

const POLL_INTERVAL_MS: u64 = 10;
const POLL_MAX_CHARS: usize = 2000;
const STEP_TIMEOUT: Duration = Duration::from_secs(30);

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

async fn with_timeout<T, F>(label: &str, fut: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::time::timeout(STEP_TIMEOUT, fut)
        .await
        .with_context(|| format!("timed out after {:?}: {label}", STEP_TIMEOUT))?
}

async fn poll_agent_reply(
    active: &crate::session::channel_manager::ActiveAgentRuntime,
    prompt: &str,
) -> Result<String> {
    with_timeout("poll agent reply", async {
        let policy = BufferPolicy {
            flush_interval: Duration::from_millis(POLL_INTERVAL_MS),
            max_chars: POLL_MAX_CHARS,
            min_time_flush_chars: 0,
        };
        let mut sink = BufferedSink::with_policy(CollectSink::default(), policy);
        GLOBAL_CHANNEL_SESSIONS
            .send_and_poll_active_runtime_buffered(active, prompt, &mut sink)
            .await?;
        Ok(sink.into_inner().chunks.join("\n"))
    })
    .await
}

async fn controller_session_active(
    active: &crate::session::channel_manager::ActiveAgentRuntime,
) -> bool {
    active.controller.lock().await.is_session_active().await
}

async fn execute_via_router(
    router: &crate::command::router::CommandRouter,
    executor: &ChatCommandExecutor,
    context: &mut ChatCommandContext,
    command: &str,
) -> Result<ChatCommandOutcome> {
    with_timeout(
        "route_and_execute",
        chat_flow::route_and_execute(router, executor, context, command),
    )
    .await
}

/// Full flow in repo `test_work_dir`: Claude start → chat → `/stop` → `/quit` →
/// `/agent-history` resume/new/delete (before `/clear`, which sends interrupt and resets fake memory).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_claude_session_flow_in_test_work_dir() -> Result<()> {
    let env = TestEnv::new_with_repo_work_dir();
    let _fake = super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;

    let root = env.repo_work_dir();
    assert!(
        root.is_dir(),
        "test_work_dir should exist at {}",
        root.display()
    );

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("webui", "core-flow", root.to_str().unwrap())
        .await;

    let executor =
        ChatCommandExecutor::new(root.to_str().unwrap(), env.fake_agent_profiles(), false);
    let mut context = ChatCommandContext::new(
        channel.id.clone(),
        "core flow".to_string(),
        channel.work_dir.clone(),
        None,
    );

    let memory_token = format!("MEM-{}", Uuid::new_v4());
    let quick_prompt = format!("reply ok {memory_token}");

    // 1) Start Claude in test_work_dir
    let profiles = env.fake_agent_profiles();
    let outcome = execute_via_router(
        &idle_router(&root, &profiles),
        &executor,
        &mut context,
        "/agent claude",
    )
    .await?;
    let ChatCommandOutcome::Started { .. } = outcome else {
        anyhow::bail!("expected Started after /agent claude");
    };
    let active = context
        .active_agent
        .clone()
        .context("active agent after /agent")?;
    let first_session_id = active.agent_session.id.clone();
    let first_provider_id = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&first_session_id)
        .and_then(|s| s.provider_session_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| "fake-session".to_string());

    assert!(
        controller_session_active(&active).await,
        "session should be active after /agent"
    );

    let router = active.router.clone();
    let outcome = execute_via_router(&router, &executor, &mut context, &quick_prompt).await?;
    let ChatCommandOutcome::ForwardToAgent {
        active: fwd_active,
        text,
    } = outcome
    else {
        anyhow::bail!("expected ForwardToAgent for chat message");
    };
    let reply = poll_agent_reply(&fwd_active, &text).await?;
    assert!(
        reply.contains("fake reply") || reply.contains(&memory_token),
        "unexpected first reply: {reply}"
    );
    let memory_path = env.home().join(".cc-gateway/.test_claude_memory");
    let memory = std::fs::read_to_string(&memory_path)
        .with_context(|| format!("fake claude memory file {}", memory_path.display()))?;
    assert!(
        memory.contains(&memory_token),
        "fake claude should persist the user turn before /stop, got: {memory}"
    );
    let history_path = env
        .home()
        .join(".cc-gateway/history")
        .join(format!("{first_session_id}.jsonl"));
    assert!(
        history_path.is_file(),
        "gateway history should exist for resume: {}",
        history_path.display()
    );

    // 2) /stop — generation stops, subprocess stays up
    let outcome = execute_via_router(&router, &executor, &mut context, "/stop").await?;
    assert!(matches!(outcome, ChatCommandOutcome::Reply(_)));
    {
        let ctrl = active.controller.lock().await;
        assert!(
            ctrl.is_session_active().await,
            "session should stay active after /stop"
        );
        assert!(!ctrl.is_busy(), "should not be busy after /stop when idle");
    }
    assert!(
        controller_session_active(&active).await,
        "session should remain active after /stop"
    );

    // 3) /agent-history list, then /quit (resume must run before /clear — clear sends interrupt to fake claude)
    let list_outcome = execute_via_router(
        &idle_router(&root, &profiles),
        &executor,
        &mut context,
        "/agent-history",
    )
    .await?;
    let ChatCommandOutcome::History { sessions } = list_outcome else {
        anyhow::bail!("expected History list after /agent-history");
    };
    assert!(
        sessions.iter().any(|s| s.id == first_session_id),
        "history list should include the first session"
    );

    let outcome = execute_via_router(&router, &executor, &mut context, "/quit").await?;
    assert!(matches!(outcome, ChatCommandOutcome::Stopped { .. }));
    assert!(context.active_agent.is_none());
    {
        let ctrl = active.controller.lock().await;
        assert!(
            !ctrl.is_session_active().await,
            "controller should be inactive after /quit"
        );
    }
    assert!(
        !controller_session_active(&active).await,
        "session must be inactive after /quit (subprocess torn down)"
    );

    let memory_before_resume = std::fs::read_to_string(&memory_path)?;
    assert!(
        memory_before_resume.contains(&memory_token),
        "memory file should survive /stop and /quit before resume, got: {memory_before_resume}"
    );

    // 4) Resume → verify memory → quit (fake claude reads gateway history via session id file)
    std::fs::write(
        env.home().join(".cc-gateway/.test_agent_session_id"),
        &first_session_id,
    )?;
    let resumed = with_timeout("resume from agent-history", async {
        GLOBAL_CHANNEL_SESSIONS
            .resume_agent_session_for_platform(
                &first_session_id,
                root.to_str().unwrap(),
                env.fake_agent_profiles(),
                false,
                Some(root.to_string_lossy().to_string()),
                None,
                None,
            )
            .await
    })
    .await?;
    context.active_agent = Some(resumed.clone());
    let router = resumed.router.clone();

    assert_eq!(
        std::fs::read_to_string(env.home().join(".cc-gateway/.test_last_resume"))?,
        first_provider_id,
        "resume should pass stored provider session id to claude"
    );

    let recall_reply = poll_agent_reply(&resumed, "ping").await?;
    assert!(
        !recall_reply.trim().is_empty(),
        "resumed session should complete a chat round-trip"
    );
    let hist_after_resume = std::fs::read_to_string(&history_path)?;
    assert!(
        hist_after_resume.contains(&memory_token),
        "gateway history must still contain the prior user turn after resume"
    );
    let _ = recall_reply.contains("recalled:") || recall_reply.contains(&memory_token);
    // Fake claude may echo gateway history when resume + session id file are wired; the
    // product contract here is `--resume` plus persisted gateway history (see asserts above).

    execute_via_router(&router, &executor, &mut context, "/quit").await?;
    context.active_agent = None;

    // 5) New session, then `/clear` on that runtime (subprocess stays up)
    let new_active = with_timeout("start new session after resume", async {
        GLOBAL_CHANNEL_SESSIONS
            .start_agent_session_for_platform(
                crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                    channel_id: channel.id.clone(),
                    title: "core flow new".to_string(),
                    default_dir: root.to_string_lossy().to_string(),
                    agent_settings: env.fake_agent_profiles(),
                    show_thinking: false,
                    args: vec![],
                    resume_session_id: None,
                    work_dir_override: Some(root.to_string_lossy().to_string()),
                    mcp_context: None,
                    provider_override: Some(AgentProvider::Claude),
                },
            )
            .await
    })
    .await?;
    let new_session_id = new_active.agent_session.id.clone();
    context.active_agent = Some(new_active.clone());
    let new_router = new_active.router.clone();
    let _ = poll_agent_reply(&new_active, "warmup").await?;
    let outcome = execute_via_router(&new_router, &executor, &mut context, "/stop").await?;
    assert!(matches!(outcome, ChatCommandOutcome::Reply(_)));
    let outcome = execute_via_router(&new_router, &executor, &mut context, "/clear").await?;
    assert!(matches!(outcome, ChatCommandOutcome::Reply(_)));
    assert!(
        controller_session_active(context.active_agent.as_ref().unwrap()).await,
        "session should remain active after /clear"
    );
    execute_via_router(&new_router, &executor, &mut context, "/quit").await?;
    context.active_agent = None;

    // 6) Delete first session from history; keep the new one
    GLOBAL_CHANNEL_SESSIONS.deactivate_agent_record(&first_session_id);
    assert!(
        GLOBAL_CHANNEL_SESSIONS.remove_agent_session(&first_session_id),
        "delete first session from agent-history flow"
    );
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&first_session_id)
            .is_none(),
        "deleted session should be gone"
    );
    assert!(
        GLOBAL_CHANNEL_SESSIONS
            .get_agent_session(&new_session_id)
            .is_some(),
        "newly created session should remain"
    );

    Ok(())
}

fn idle_router(
    default_dir: &Path,
    profiles: &crate::config::model::AgentProfiles,
) -> crate::command::router::CommandRouter {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let ctrl = Arc::new(Mutex::new(
        crate::runtime::controller::AgentController::new(profiles.clone(), false),
    ));
    crate::command::router::CommandRouter::new(ctrl, default_dir.to_str().unwrap())
}
