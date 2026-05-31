use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::db;
use crate::runtime::event_poller::{BufferedSink, EventPollSink};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::channel_model::AgentSessionState;

use super::helpers::TestEnv;

struct CollectSink {
    text: String,
}

#[async_trait::async_trait]
impl EventPollSink for CollectSink {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        self.text.push_str(text);
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

struct ChunkSink {
    chunks: Vec<String>,
}

#[async_trait::async_trait]
impl EventPollSink for ChunkSink {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        self.chunks.push(text.to_string());
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

fn create_thinking_fake_claude(home: &Path) -> PathBuf {
    let script = home.join("claude");
    std::fs::write(
        &script,
        r#"#!/bin/sh
session_id="fake-thinking-session"
mkdir -p "$HOME/.claude/sessions"
printf '{"sessionId":"%s"}\n' "$session_id" > "$HOME/.claude/sessions/$$.json"
while IFS= read -r line; do
  printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"private chain of thought"},{"type":"text","text":"visible answer"}]}}\n'
  printf '{"type":"result","result":"visible answer","usage":{"input_tokens":1,"output_tokens":2}}\n'
done
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }
    script
}

fn create_cwd_fake_claude(home: &Path) -> PathBuf {
    let script = home.join("claude");
    std::fs::write(
        &script,
        r#"#!/bin/sh
mkdir -p "$HOME/.claude/sessions"
pwd > "$HOME/claude-cwd.txt"
printf '{"sessionId":"fake-cwd-session"}\n' > "$HOME/.claude/sessions/$$.json"
while IFS= read -r line; do
  printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"cwd reply"}]}}\n'
  printf '{"type":"result","result":"cwd reply","usage":{"input_tokens":1,"output_tokens":2}}\n'
done
"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }
    script
}

async fn collect_thinking_flow(show_thinking: bool) -> Result<Vec<String>> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join(if show_thinking {
        "thinking-visible"
    } else {
        "thinking-hidden"
    });
    std::fs::create_dir_all(&work_dir)?;
    let mut config = env.fake_agent_profiles();
    create_thinking_fake_claude(env.home());

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel(
            if show_thinking {
                "thinking-visible"
            } else {
                "thinking-hidden"
            },
            "core-thinking-flow",
            work_dir.to_str().unwrap(),
        )
        .await;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: channel.id.clone(),
                title: "Thinking Flow".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: config,
                show_thinking,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let mut sink = BufferedSink::new(
        ChunkSink { chunks: Vec::new() },
        std::time::Duration::from_millis(10),
        2000,
    );
    GLOBAL_CHANNEL_SESSIONS
        .send_and_poll_active_runtime_buffered(&active, "hello", &mut sink)
        .await?;
    {
        let ctrl = active.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;

    Ok(sink.into_inner().chunks)
}

#[tokio::test]
async fn core_session_lifecycle_with_fake_claude_updates_db_and_resumes() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("project");
    std::fs::create_dir_all(&work_dir)?;
    let fake_config = env.fake_agent_profiles();

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("tui", "core-flow", work_dir.to_str().unwrap())
        .await;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: channel.id.clone(),
                title: "Core Flow".to_string(),
                default_dir: work_dir.to_string_lossy().to_string(),
                agent_settings: fake_config.clone(),
                show_thinking: false,
                args: vec![],
                resume_session_id: None,
                work_dir_override: None,
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;

    assert_eq!(db::load_all_channel_sessions().len(), 1);
    let stored = db::load_agent_sessions_by_channel_id(&channel.id);
    assert_eq!(stored.len(), 1);
    assert!(stored[0].active);

    let mut buffered = BufferedSink::new(
        CollectSink {
            text: String::new(),
        },
        std::time::Duration::from_millis(10),
        2000,
    );
    GLOBAL_CHANNEL_SESSIONS
        .send_and_poll_active_runtime_buffered(&active, "hello", &mut buffered)
        .await?;
    let sink = buffered.into_inner();
    assert!(sink.text.contains("fake reply"));

    {
        let ctrl = active.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;
    let stopped = db::load_agent_sessions_by_channel_id(&channel.id);
    assert!(!stopped[0].active);
    assert_eq!(stopped[0].state, AgentSessionState::Stopped);

    GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
    GLOBAL_CHANNEL_SESSIONS.load_from_db();
    let restored = GLOBAL_CHANNEL_SESSIONS.list_agent_sessions_by_channel(&channel.id, None);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].work_dir, work_dir.to_string_lossy());

    let resumed = GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_runtime(
            &restored[0].id,
            work_dir.to_str().unwrap(),
            fake_config,
            false,
        )
        .await?;
    {
        let ctrl = resumed.controller.lock().await;
        assert!(ctrl.is_session_active().await);
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;

    Ok(())
}

#[tokio::test]
async fn start_session_uses_work_dir_override_as_process_cwd_and_persisted_work_dir() -> Result<()>
{
    let env = TestEnv::new();
    db::init_schema()?;
    let root = env.home().join("override-root");
    let child = root.join("child");
    std::fs::create_dir_all(&child)?;
    let mut config = env.fake_agent_profiles();
    create_cwd_fake_claude(env.home());

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "override-flow", root.to_str().unwrap())
        .await;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: channel.id.clone(),
                title: "Override Flow".to_string(),
                default_dir: root.to_string_lossy().to_string(),
                agent_settings: config,
                show_thinking: false,
                args: vec![],
                resume_session_id: None,
                work_dir_override: Some(child.to_string_lossy().to_string()),
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;

    let process_cwd = std::fs::read_to_string(env.home().join("claude-cwd.txt"))?;
    assert_eq!(process_cwd.trim(), child.to_string_lossy());
    assert_eq!(active.agent_session.work_dir, child.to_string_lossy());
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_active_agent_session(&channel.id)
            .unwrap()
            .work_dir,
        child.to_string_lossy()
    );

    {
        let ctrl = active.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;

    Ok(())
}

#[tokio::test]
async fn resume_session_uses_original_session_work_dir_even_if_channel_dir_changed() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let original = env.home().join("resume-original");
    let current = env.home().join("resume-current");
    std::fs::create_dir_all(&original)?;
    std::fs::create_dir_all(&current)?;
    let mut config = env.fake_agent_profiles();
    create_cwd_fake_claude(env.home());

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "resume-workdir", current.to_str().unwrap())
        .await;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_agent_session_for_platform(
            crate::session::channel_manager::StartAgentSessionForPlatformArgs {
                channel_id: channel.id.clone(),
                title: "Resume WorkDir".to_string(),
                default_dir: current.to_string_lossy().to_string(),
                agent_settings: config.clone(),
                show_thinking: false,
                args: vec![],
                resume_session_id: None,
                work_dir_override: Some(original.to_string_lossy().to_string()),
                mcp_context: None,
                provider_override: None,
            },
        )
        .await?;
    let session_id = active.agent_session.id.clone();
    {
        let ctrl = active.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;
    GLOBAL_CHANNEL_SESSIONS
        .switch_work_dir(&channel.id, current.clone())
        .await?;

    let resumed = GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_for_platform(
            &session_id,
            current.to_str().unwrap(),
            config,
            false,
            Some(current.to_string_lossy().to_string()),
            None,
        )
        .await?;

    let process_cwd = std::fs::read_to_string(env.home().join("claude-cwd.txt"))?;
    assert_eq!(process_cwd.trim(), original.to_string_lossy());
    assert_eq!(resumed.agent_session.work_dir, original.to_string_lossy());
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_channel(&channel.id)
            .unwrap()
            .work_dir,
        original.to_string_lossy()
    );

    {
        let ctrl = resumed.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;

    Ok(())
}

fn create_tagged_fake_cli(home: &Path, tag: &str) -> PathBuf {
    // Map provider tags to CLI binary names so resolve_cli_path can find them.
    let name = match tag {
        "cursor" => "agent",
        _ => tag,
    };
    let script = home.join(name);
    std::fs::write(
        &script,
        format!(
            r#"#!/bin/sh
mkdir -p "$HOME/.claude/sessions"
echo "{tag}" > "$HOME/last-spawn-tag.txt"
pwd > "$HOME/claude-cwd.txt"
printf '{{"sessionId":"fake-{tag}-session"}}\n' > "$HOME/.claude/sessions/$$.json"
while IFS= read -r line; do
  printf '{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"tag reply"}}]}}}}\n'
  printf '{{"type":"result","result":"tag reply","usage":{{"input_tokens":1,"output_tokens":2}}}}\n'
done
"#
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).unwrap();
    }
    script
}

#[tokio::test]
async fn controller_start_session_with_explicit_claude_provider() -> Result<()> {
    use crate::config::model::{AgentProfiles, AgentProvider, AgentProviderConfig};
    use crate::runtime::controller::AgentController;

    let env = TestEnv::new();
    let work_dir = env.home().join("controller-claude");
    std::fs::create_dir_all(&work_dir)?;
    let claude_cli = create_tagged_fake_cli(env.home(), "claude");
    let cursor_cli = create_tagged_fake_cli(env.home(), "cursor");
    let settings = AgentProfiles {
        default: AgentProvider::Cursor,
        claude: AgentProviderConfig {
            default_args: Some(String::new()),
            ..Default::default()
        },
        cursor: AgentProviderConfig {
            default_args: Some(String::new()),
            ..Default::default()
        },
        ..Default::default()
    };

    let controller = AgentController::new(settings, false);
    {
        let ctrl = std::sync::Arc::new(tokio::sync::Mutex::new(controller));
        let c = ctrl.lock().await;
        c.init_work_dir(work_dir.to_string_lossy().to_string())
            .await;
        c.set_pending_resume_session_id(Some("fake-claude-session".to_string()))
            .await;
        c.start_session_with_provider(
            work_dir.to_string_lossy().to_string(),
            vec![],
            Some(AgentProvider::Claude),
        )
        .await?;
        c.stop_session().await?;
    }

    let tag = std::fs::read_to_string(env.home().join("last-spawn-tag.txt"))?;
    assert_eq!(tag.trim(), "claude");
    Ok(())
}

#[tokio::test]
async fn resume_session_uses_stored_provider_not_agent_default() -> Result<()> {
    use crate::config::model::{AgentProfiles, AgentProvider, AgentProviderConfig};
    use crate::session::channel_model::{AgentSession, AgentSessionState};

    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("provider-resume");
    std::fs::create_dir_all(&work_dir)?;

    let _claude_cli = create_tagged_fake_cli(env.home(), "claude");
    let _cursor_cli = create_tagged_fake_cli(env.home(), "cursor");
    let agent_settings = AgentProfiles::default();

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "provider-resume", work_dir.to_str().unwrap())
        .await;

    let mut session = AgentSession::new(&channel.id, "Stored Provider", work_dir.to_str().unwrap());
    session.provider = "claude".to_string();
    session.state = AgentSessionState::Stopped;
    session.provider_session_id = Some("fake-claude-session".to_string());
    let session_id = session.id.clone();
    db::insert_agent_session(&session);
    GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
    GLOBAL_CHANNEL_SESSIONS.load_from_db();

    let loaded = GLOBAL_CHANNEL_SESSIONS
        .get_agent_session(&session_id)
        .expect("session loaded from db");
    assert_eq!(loaded.provider, "claude");
    let resume_cfg = agent_settings.config_for_provider(Some(loaded.stored_provider()));
    assert_eq!(resume_cfg.provider, AgentProvider::Claude);
    assert_eq!(resume_cfg.cli_path, "claude");

    let all = crate::db::load_all_agent_sessions();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].provider, "claude");
    assert_eq!(all[0].id, session_id);

    let tag_path = env.home().join("last-spawn-tag.txt");
    let _ = std::fs::remove_file(&tag_path);

    let resumed = GLOBAL_CHANNEL_SESSIONS
        .resume_agent_session_for_platform(
            &session_id,
            work_dir.to_str().unwrap(),
            agent_settings,
            false,
            None,
            None,
        )
        .await?;

    let tag = std::fs::read_to_string(&tag_path)?;
    assert_eq!(tag.trim(), "claude");
    assert_eq!(resumed.agent_session.provider, "claude");

    {
        let ctrl = resumed.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;

    Ok(())
}

#[tokio::test]
async fn hidden_thinking_shows_placeholder_without_private_content() -> Result<()> {
    let chunks = collect_thinking_flow(false).await?;

    assert!(chunks.iter().any(|chunk| chunk == "💭 Thinking..."));
    assert!(!chunks
        .iter()
        .any(|chunk| chunk.contains("private chain of thought")));
    assert!(chunks.iter().any(|chunk| chunk.contains("visible answer")));

    Ok(())
}

#[tokio::test]
async fn shown_thinking_includes_private_content() -> Result<()> {
    let chunks = collect_thinking_flow(true).await?;

    assert!(chunks
        .iter()
        .any(|chunk| chunk.contains("private chain of thought")));
    assert!(chunks.iter().any(|chunk| chunk.contains("visible answer")));

    Ok(())
}
