//! Pi session restart: gateway may reactivate a record, but Pi CLI does not restore prior chats.

use anyhow::Result;

use crate::config::model::AgentProvider;
use crate::db;
use crate::history::recorder::append_session_history;
use crate::session::channel_manager::{
    GLOBAL_CHANNEL_SESSIONS, StartAgentSessionForPlatformArgs,
};

use super::helpers::{create_fake_pi_cli, TestEnv};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_resume_skips_switch_session_starts_fresh() -> Result<()> {
    let env = TestEnv::new();
    create_fake_pi_cli(env.home());
    db::init_schema()?;

    let work_dir = env.home().join("pi-resume");
    std::fs::create_dir_all(&work_dir)?;
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "pi-resume-chat", work_dir.to_str().unwrap())
        .await;

    let mut profiles = env.fake_agent_profiles();
    profiles.pi.enabled = true;

    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(StartAgentSessionForPlatformArgs {
            channel_id: channel.id.clone(),
            title: "Pi resume".to_string(),
            default_dir: work_dir.to_string_lossy().to_string(),
            agent_settings: profiles.clone(),
            show_thinking: false,
            args: vec![],
            resume_session_id: None,
            work_dir_override: Some(work_dir.to_string_lossy().to_string()),
            mcp_context: None,
            provider_override: Some(AgentProvider::Pi),
        })
        .await?;

    let session_id = active.agent_session.id.clone();
    let pi_session_file = active
        .agent_session
        .provider_session_id
        .clone()
        .expect("Pi spawn should persist sessionFile via get_state");
    assert!(
        pi_session_file.ends_with(".jsonl"),
        "provider_session_id should be Pi sessionFile path, got {pi_session_file}"
    );

    append_session_history(&session_id, "user", "hello pi")?;

    GLOBAL_CHANNEL_SESSIONS
        .stop_active_runtime_for_channel(&channel.id, Some(&active))
        .await?;

    let switch_marker = env.home().join(".cc-gateway/.test_last_pi_switch_session");
    let _ = std::fs::remove_file(&switch_marker);

    let _resumed = GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_for_platform(
            &session_id,
            work_dir.to_str().unwrap(),
            profiles,
            false,
            None,
            None,
            None,
        )
        .await?;

    assert!(
        !switch_marker.exists(),
        "Pi resume must not call switch_session while restore is unsupported"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_resume_succeeds_without_switch_session_even_when_marker_would_fail() -> Result<()> {
    let env = TestEnv::new();
    create_fake_pi_cli(env.home());
    db::init_schema()?;

    let work_dir = env.home().join("pi-resume-fail");
    std::fs::create_dir_all(&work_dir)?;
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "pi-resume-fail-chat", work_dir.to_str().unwrap())
        .await;

    let mut profiles = env.fake_agent_profiles();
    profiles.pi.enabled = true;

    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(StartAgentSessionForPlatformArgs {
            channel_id: channel.id.clone(),
            title: "Pi resume fail".to_string(),
            default_dir: work_dir.to_string_lossy().to_string(),
            agent_settings: profiles.clone(),
            show_thinking: false,
            args: vec![],
            resume_session_id: None,
            work_dir_override: Some(work_dir.to_string_lossy().to_string()),
            mcp_context: None,
            provider_override: Some(AgentProvider::Pi),
        })
        .await?;

    let session_id = active.agent_session.id.clone();
    append_session_history(&session_id, "user", "hello")?;

    GLOBAL_CHANNEL_SESSIONS
        .stop_active_runtime_for_channel(&channel.id, Some(&active))
        .await?;

    std::fs::write(env.home().join(".cc-gateway/.test_pi_fail_switch"), "1")?;

    GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_for_platform(
            &session_id,
            work_dir.to_str().unwrap(),
            profiles,
            false,
            None,
            None,
            None,
        )
        .await?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_spawn_strips_no_session_from_profile_and_extra_args() -> Result<()> {
    let env = TestEnv::new();
    create_fake_pi_cli(env.home());
    db::init_schema()?;

    let work_dir = env.home().join("pi-no-session-strip");
    std::fs::create_dir_all(&work_dir)?;
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "pi-strip-chat", work_dir.to_str().unwrap())
        .await;

    let mut profiles = env.fake_agent_profiles();
    profiles.pi.enabled = true;
    profiles.pi.default_args = Some("--no-session".to_string());

    GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(StartAgentSessionForPlatformArgs {
            channel_id: channel.id.clone(),
            title: "Pi strip no-session".to_string(),
            default_dir: work_dir.to_string_lossy().to_string(),
            agent_settings: profiles,
            show_thinking: false,
            args: vec!["--no-session".to_string()],
            resume_session_id: None,
            work_dir_override: Some(work_dir.to_string_lossy().to_string()),
            mcp_context: None,
            provider_override: Some(AgentProvider::Pi),
        })
        .await?;

    let argv = std::fs::read_to_string(env.home().join(".cc-gateway/.test_pi_argv"))?;
    assert!(
        !argv.contains("--no-session"),
        "Pi spawn must strip --no-session to keep session persistence, argv: {argv}"
    );

    Ok(())
}
