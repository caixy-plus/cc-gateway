//! Feishu [`EventPollSink`] for the persistent session poller.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::platform::feishu::FeishuPlatform;
use crate::runtime::event_poller::EventPollSink;

/// Delivers agent stream events to a Feishu chat.
pub(crate) struct FeishuEventSink {
    pub platform: FeishuPlatform,
    pub receive_id_type: String,
    pub receive_id: String,
    pub chat_id_str: String,
    pub sender_open_id: Arc<RwLock<String>>,
}

#[async_trait]
impl EventPollSink for FeishuEventSink {
    async fn flush(&mut self, text: &str, _is_done: bool) -> Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }
        self.platform
            .send_text_message(&self.receive_id_type, &self.receive_id, text)
            .await?;
        crate::web::state::broadcast_event(
            &self.chat_id_str,
            "feishu",
            &self.chat_id_str,
            "assistant",
            text,
        );
        Ok(())
    }

    async fn on_permission_request(
        &mut self,
        request_id: &str,
        tool_name: &str,
        input: Option<&serde_json::Value>,
    ) -> Result<()> {
        let sender_open_id = self.sender_open_id.read().await.clone();
        self.platform.pending_permissions.insert(
            request_id.to_string(),
            crate::platform::feishu::PendingPermissionContext {
                request_id: request_id.to_string(),
                tool_name: tool_name.to_string(),
                chat_id: self.chat_id_str.clone(),
                sender_open_id,
                input: input.cloned(),
                created_at: std::time::Instant::now(),
            },
        );
        let card = crate::platform::feishu::cards::build_permission_card(
            request_id,
            tool_name,
            &self.chat_id_str,
        );
        self.platform
            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
            .await?;
        crate::web::state::broadcast_event(
            &self.chat_id_str,
            "feishu",
            &self.chat_id_str,
            "system",
            &crate::t_fmt!(
                "feishu.permission_request_text",
                NAME = tool_name,
                ID = request_id
            ),
        );
        Ok(())
    }

    async fn on_confirm_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        let card = crate::platform::feishu::cards::build_select_card(
            request_id,
            prompt,
            options,
            &self.chat_id_str,
        );
        self.platform
            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
            .await?;
        Ok(())
    }

    async fn on_select_request(
        &mut self,
        request_id: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<()> {
        let card = crate::platform::feishu::cards::build_select_card(
            request_id,
            prompt,
            options,
            &self.chat_id_str,
        );
        self.platform
            .send_interactive_card(&self.receive_id_type, &self.receive_id, &card)
            .await?;
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
