use crate::config::model::{AgentProfiles, AgentProvider};
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

use super::helpers::TestEnv;

#[tokio::test]
async fn channel_default_provider_overrides_global_default() {
    let env = TestEnv::new();
    let mut profiles = AgentProfiles::default();
    profiles.default = AgentProvider::Cursor;

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("test", "ch-test", env.home().to_str().unwrap())
        .await;
    GLOBAL_CHANNEL_SESSIONS
        .set_channel_default_provider(&channel.id, AgentProvider::Claude)
        .unwrap();
    let channel_id = channel.id.as_str();

    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS.effective_channel_provider(channel_id, &profiles),
        AgentProvider::Claude
    );
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS.resolve_start_provider(channel_id, &profiles, None),
        AgentProvider::Claude
    );
    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS.resolve_start_provider(
            channel_id,
            &profiles,
            Some(AgentProvider::Cursor)
        ),
        AgentProvider::Cursor
    );
}
