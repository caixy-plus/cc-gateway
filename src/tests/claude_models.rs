//! Claude `/models` list + in-session switch via forwarded `/model`.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::command::models::claude_model_alias_fallback;
use crate::config::model::AgentProvider;
use crate::db;
use crate::session::channel_command::{ChatCommandContext, ChatCommandExecutor, ChatCommandOutcome};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::chat_flow;

use super::helpers::TestEnv;

const STEP_TIMEOUT: Duration = Duration::from_secs(30);

async fn with_timeout<T, F>(label: &str, fut: F) -> Result<T>
where
    F: std::future::Future<Output = Result<T>>,
{
    tokio::time::timeout(STEP_TIMEOUT, fut)
        .await
        .with_context(|| format!("timed out after {:?}: {label}", STEP_TIMEOUT))?
}

fn idle_router(
    default_dir: &std::path::Path,
    profiles: &crate::config::model::AgentProfiles,
) -> crate::command::router::CommandRouter {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    let ctrl = Arc::new(Mutex::new(
        crate::runtime::controller::AgentController::new(profiles.clone(), false),
    ));
    crate::command::router::CommandRouter::new(ctrl, default_dir.to_str().unwrap())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claude_models_lists_curated_aliases() -> Result<()> {
    let env = TestEnv::new_with_repo_work_dir();
    let _fake = super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;

    let root = env.repo_work_dir();
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("webui", "claude-models", root.to_str().unwrap())
        .await;

    let profiles = env.fake_agent_profiles();
    let executor = ChatCommandExecutor::new(root.to_str().unwrap(), profiles.clone(), false);
    let mut context = ChatCommandContext::new(
        channel.id.clone(),
        "claude models".to_string(),
        channel.work_dir.clone(),
        None,
    );

    let started = with_timeout(
        "start claude",
        chat_flow::route_and_execute(
            &idle_router(&root, &profiles),
            &executor,
            &mut context,
            "/agent claude",
        ),
    )
    .await?;
    let ChatCommandOutcome::Started { .. } = started else {
        if let ChatCommandOutcome::Error(msg) = &started {
            anyhow::bail!("expected Started after /agent claude, got error: {msg}");
        }
        anyhow::bail!("expected Started after /agent claude");
    };
    let active = context
        .active_agent
        .clone()
        .context("active agent after /agent")?;
    let router = active.router.clone();

    let outcome = with_timeout(
        "list models",
        chat_flow::route_and_execute(&router, &executor, &mut context, "/models"),
    )
    .await?;
    let ChatCommandOutcome::SelectModel { options, .. } = outcome else {
        anyhow::bail!("expected SelectModel after /models");
    };
    assert!(
        options.contains(&"sonnet".to_string()),
        "discovered list should include sonnet fallback, got {options:?}"
    );
    assert!(
        options.len() >= claude_model_alias_fallback().len(),
        "list should include alias fallback entries"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claude_models_switch_unknown_alias_passthrough() -> Result<()> {
    let env = TestEnv::new_with_repo_work_dir();
    let _fake = super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;

    let root = env.repo_work_dir();
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("webui", "claude-passthrough", root.to_str().unwrap())
        .await;

    let profiles = env.fake_agent_profiles();
    let executor = ChatCommandExecutor::new(root.to_str().unwrap(), profiles.clone(), false);
    let mut context = ChatCommandContext::new(
        channel.id.clone(),
        "claude passthrough".to_string(),
        channel.work_dir.clone(),
        None,
    );

    let started = with_timeout(
        "start claude",
        chat_flow::route_and_execute(
            &idle_router(&root, &profiles),
            &executor,
            &mut context,
            "/agent claude",
        ),
    )
    .await?;
    let ChatCommandOutcome::Started { .. } = started else {
        anyhow::bail!("expected Started after /agent claude");
    };

    let active = context.active_agent.clone().context("active agent")?;
    let router = active.router.clone();

    let outcome = with_timeout(
        "switch unknown alias",
        chat_flow::route_and_execute(&router, &executor, &mut context, "/models claude-opus-4-9"),
    )
    .await?;
    let ChatCommandOutcome::Reply(reply) = outcome else {
        anyhow::bail!("expected Reply after passthrough switch");
    };
    assert!(
        reply.contains("claude-opus-4-9"),
        "should confirm passthrough model id: {reply}"
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn claude_models_switch_forwards_model_command() -> Result<()> {
    let env = TestEnv::new_with_repo_work_dir();
    let _fake = super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;

    let root = env.repo_work_dir();
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("webui", "claude-switch", root.to_str().unwrap())
        .await;

    let profiles = env.fake_agent_profiles();
    let executor = ChatCommandExecutor::new(root.to_str().unwrap(), profiles.clone(), false);
    let mut context = ChatCommandContext::new(
        channel.id.clone(),
        "claude switch".to_string(),
        channel.work_dir.clone(),
        None,
    );

    let started = with_timeout(
        "start claude",
        chat_flow::route_and_execute(
            &idle_router(&root, &profiles),
            &executor,
            &mut context,
            "/agent claude",
        ),
    )
    .await?;
    let ChatCommandOutcome::Started { .. } = started else {
        if let ChatCommandOutcome::Error(msg) = &started {
            anyhow::bail!("expected Started after /agent claude, got error: {msg}");
        }
        anyhow::bail!("expected Started after /agent claude");
    };

    let active = context
        .active_agent
        .clone()
        .context("active agent after /agent")?;
    let router = active.router.clone();

    let outcome = with_timeout(
        "switch model",
        chat_flow::route_and_execute(&router, &executor, &mut context, "/models sonnet"),
    )
    .await?;
    let ChatCommandOutcome::Reply(reply) = outcome else {
        anyhow::bail!("expected Reply after /models sonnet");
    };
    assert!(
        reply.contains("sonnet"),
        "switch confirmation should mention sonnet: {reply}"
    );

    let ctrl = active.controller.lock().await;
    assert_eq!(
        ctrl.current_model_id().await.as_deref(),
        Some("sonnet"),
        "controller should cache switched model"
    );

    let caps = crate::config::agent_registry::capabilities_for(&AgentProvider::Claude);
    assert!(caps.model_switch_via_user_message);
    Ok(())
}
