//! MCP `send_file` attach at agent spawn.

use anyhow::Result;

use crate::agent::mcp_attach::{prepare_cursor_mcp, prepare_pi_mcp};
use crate::config::model::AgentProvider;
use crate::db;
use crate::runtime::file_delivery::{FeishuFileTarget, McpDeliveryTarget};
use crate::runtime::mcp_server::McpContext;
use crate::session::channel_manager::{
    CreateAndStartAgentSessionArgs, GLOBAL_CHANNEL_SESSIONS, StartAgentSessionForPlatformArgs,
};

use super::helpers::TestEnv;

fn sample_mcp_context() -> McpContext {
    McpContext {
        delivery: McpDeliveryTarget::Feishu(FeishuFileTarget {
            app_id: "app".to_string(),
            app_secret: "secret".to_string(),
            chat_id: "chat".to_string(),
            receive_id_type: "open_id".to_string(),
        }),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn start_agent_session_injects_claude_mcp_config() -> Result<()> {
    let env = TestEnv::new();
    let _fake = super::helpers::create_fake_agent_cli(env.home());
    db::init_schema()?;

    let work_dir = env.home().join("mcp-spawn");
    std::fs::create_dir_all(&work_dir)?;
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "mcp-chat", work_dir.to_str().unwrap())
        .await;

    let (_session, _controller) = GLOBAL_CHANNEL_SESSIONS
        .create_and_start_agent_session(CreateAndStartAgentSessionArgs {
            channel_id: channel.id.clone(),
            title: "MCP spawn".to_string(),
            default_dir: work_dir.to_string_lossy().to_string(),
            agent_settings: env.fake_agent_profiles(),
            show_thinking: false,
            args: vec![],
            resume_session_id: None,
            work_dir_override: Some(work_dir.to_string_lossy().to_string()),
            mcp_context: Some(sample_mcp_context()),
            provider_override: Some(AgentProvider::Claude),
        })
        .await?;

    let mcp_marker = env.home().join(".cc-gateway/.test_last_mcp_config");
    assert!(
        mcp_marker.is_file(),
        "Claude spawn should pass --mcp-config when mcp_context is set"
    );
    let path = std::fs::read_to_string(&mcp_marker)?;
    let body = std::fs::read_to_string(path.trim())?;
    assert!(
        body.contains("cc-gateway") && body.contains("_mcp-server"),
        "mcp config should reference cc-gateway _mcp-server, got: {body}"
    );

    Ok(())
}

#[tokio::test]
async fn cursor_prepare_writes_project_mcp_json() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("cc-gateway-cursor-mcp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let ctx = sample_mcp_context();

    let servers = prepare_cursor_mcp(dir.to_str().unwrap(), Some(&ctx)).await?;
    let path = dir.join(".cursor/mcp.json");
    assert!(path.is_file(), "Cursor should load MCP from .cursor/mcp.json");
    let body = std::fs::read_to_string(&path)?;
    assert!(body.contains("cc-gateway"));
    assert!(servers.as_array().is_some_and(|a| !a.is_empty()));

    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[tokio::test]
async fn pi_prepare_writes_project_mcp_json() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("cc-gateway-pi-mcp-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)?;
    let ctx = sample_mcp_context();

    prepare_pi_mcp(dir.to_str().unwrap(), Some(&ctx)).await?;
    let path = dir.join(".pi/mcp.json");
    assert!(path.is_file(), "Pi should load MCP from .pi/mcp.json");
    let body = std::fs::read_to_string(&path)?;
    assert!(body.contains("cc-gateway"));

    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pi_start_agent_session_writes_mcp_json_in_work_dir() -> Result<()> {
    let env = TestEnv::new();
    let _fake = super::helpers::create_fake_pi_cli(env.home());
    db::init_schema()?;

    let work_dir = env.home().join("pi-mcp-spawn");
    std::fs::create_dir_all(&work_dir)?;
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "pi-mcp-chat", work_dir.to_str().unwrap())
        .await;

    let mut profiles = env.fake_agent_profiles();
    profiles.pi.enabled = true;

    GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(StartAgentSessionForPlatformArgs {
            channel_id: channel.id.clone(),
            title: "Pi MCP".to_string(),
            default_dir: work_dir.to_string_lossy().to_string(),
            agent_settings: profiles,
            show_thinking: false,
            args: vec![],
            resume_session_id: None,
            work_dir_override: Some(work_dir.to_string_lossy().to_string()),
            mcp_context: Some(sample_mcp_context()),
            provider_override: Some(AgentProvider::Pi),
        })
        .await?;

    let mcp_path = work_dir.join(".pi/mcp.json");
    assert!(
        mcp_path.is_file(),
        "Pi spawn should write .pi/mcp.json when mcp_context is set"
    );

    Ok(())
}
