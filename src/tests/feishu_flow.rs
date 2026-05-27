use crate::runtime::file_delivery::McpDeliveryTarget;
use crate::command::builtin::list_directory_paths;
use crate::platform::feishu::cards::build_dir_picker_card;
use crate::platform::feishu::FeishuChannelRuntime;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

use super::helpers::{feishu_platform, feishu_text_event, TestEnv};

#[test]
fn feishu_normalizes_private_text_message_for_open_id_replies() {
    let platform = feishu_platform("~");
    let event = feishu_text_event("msg-1", "oc-chat", "p2p", "ou-user", "/pwd");

    let normalized = platform
        .normalize_im_event(&event)
        .expect("message should normalize");

    assert_eq!(normalized.message_id, "msg-1");
    assert_eq!(normalized.content, "/pwd");
    assert_eq!(normalized.chat_id.as_deref(), Some("oc-chat"));
    assert_eq!(normalized.receive_id_type, "open_id");
    assert_eq!(normalized.receive_id, "ou-user");
}

#[test]
fn feishu_mcp_context_targets_current_receive_id() {
    let platform = feishu_platform("~");
    let context = platform.mcp_context_for_receive("ou-user", "open_id");

    match context.delivery {
        McpDeliveryTarget::Feishu(target) => {
            assert_eq!(target.app_id, "app-id");
            assert_eq!(target.app_secret, "app-secret");
            assert_eq!(target.chat_id, "ou-user");
            assert_eq!(target.receive_id_type, "open_id");
        }
        other => panic!("expected Feishu target, got {:?}", other),
    }
}

#[tokio::test]
async fn feishu_ll_card_and_directory_selection_state_are_offline_testable() {
    let env = TestEnv::new();
    crate::db::init_schema().unwrap();
    let root = env.home().join("project");
    let child = root.join("child");
    std::fs::create_dir_all(&child).unwrap();

    let dirs = list_directory_paths(root.to_str().unwrap()).unwrap();
    let card = build_dir_picker_card(
        &dirs,
        0,
        root.to_str().unwrap(),
        "oc-chat",
        "open_id",
        "ou-user",
    );
    let first_value = &card["body"]["elements"][1]["behaviors"][0]["value"];
    assert_eq!(first_value["cmd"].as_str(), Some("cd"));
    assert_eq!(first_value["chat_id"].as_str(), Some("oc-chat"));
    assert_eq!(first_value["receive_id"].as_str(), Some("ou-user"));

    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("feishu", "oc-chat", root.to_str().unwrap())
        .await;
    GLOBAL_CHANNEL_SESSIONS
        .switch_work_dir(&channel.id, child.clone())
        .await
        .unwrap();
    let mut runtime =
        FeishuChannelRuntime::new(channel, "open_id".to_string(), "ou-user".to_string());
    runtime.set_work_dir(child.to_string_lossy().to_string());

    assert_eq!(
        GLOBAL_CHANNEL_SESSIONS
            .get_channel(&runtime.channel_session.id)
            .unwrap()
            .work_dir,
        child.to_string_lossy().to_string()
    );
    assert_eq!(
        runtime.channel_session.work_dir,
        child.to_string_lossy().to_string()
    );
}
