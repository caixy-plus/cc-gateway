use anyhow::Result;

use crate::claude::event_poller::{ClaudeEventPoller, EventPollSink};
use crate::db;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::session::channel_model::ClaudeSessionState;

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
        _questions: &[crate::claude::controller::QuestionItem],
    ) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn core_session_lifecycle_with_fake_claude_updates_db_and_resumes() -> Result<()> {
    let env = TestEnv::new();
    db::init_schema()?;
    let work_dir = env.home().join("project");
    std::fs::create_dir_all(&work_dir)?;
    let fake_config = env.fake_claude_config();

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("tui", "core-flow", work_dir.to_str().unwrap())
        .await;
    let active = GLOBAL_CHANNEL_SESSIONS
        .start_claude_session_for_platform(
            &channel.id,
            "Core Flow",
            work_dir.to_str().unwrap(),
            fake_config.clone(),
            false,
            vec![],
            None,
            None,
            None,
        )
        .await?;

    assert_eq!(db::load_all_channel_sessions().len(), 1);
    let stored = db::load_claude_sessions_by_channel_id(&channel.id);
    assert_eq!(stored.len(), 1);
    assert!(stored[0].active);

    {
        let ctrl = active.controller.lock().await;
        ctrl.send_message("hello").await?;
    }
    let poller = {
        let ctrl = active.controller.lock().await;
        ClaudeEventPoller::from_controller(&ctrl)
    };
    let mut sink = CollectSink {
        text: String::new(),
    };
    poller.run(&mut sink).await?;
    assert!(sink.text.contains("fake reply"));

    {
        let ctrl = active.controller.lock().await;
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;
    let stopped = db::load_claude_sessions_by_channel_id(&channel.id);
    assert!(!stopped[0].active);
    assert_eq!(stopped[0].state, ClaudeSessionState::Stopped);

    GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
    GLOBAL_CHANNEL_SESSIONS.load_from_db();
    let restored = GLOBAL_CHANNEL_SESSIONS.list_claude_sessions_by_channel(&channel.id, None);
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].work_dir, work_dir.to_string_lossy());

    let (_resumed_session, resumed_controller) = GLOBAL_CHANNEL_SESSIONS
        .resume_claude_session(&restored[0].id, fake_config, false)
        .await?;
    {
        let ctrl = resumed_controller.lock().await;
        assert!(ctrl.is_session_active().await);
        ctrl.stop_session().await?;
    }
    GLOBAL_CHANNEL_SESSIONS
        .stop_channel_session(&channel.id)
        .await?;

    Ok(())
}
