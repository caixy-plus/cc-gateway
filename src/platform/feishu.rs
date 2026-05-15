use anyhow::{Context, Result};
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

use crate::claude::controller::{ClaudeController, ControllerEvent};
use crate::command::router::CommandRouter;
use crate::config::model::FeishuConfig;

#[derive(Clone)]
pub struct FeishuPlatform {
    config: FeishuConfig,
    router: Arc<CommandRouter>,
    controller: Arc<Mutex<ClaudeController>>,
    http_client: reqwest::Client,
}

impl FeishuPlatform {
    pub fn new(
        config: FeishuConfig,
        router: Arc<CommandRouter>,
        controller: Arc<Mutex<ClaudeController>>,
    ) -> Self {
        Self {
            config,
            router,
            controller,
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting Feishu platform...");

        // Get tenant access token
        let token = self.get_tenant_access_token().await?;
        info!("Feishu tenant access token obtained");

        // For simplicity, use long-polling via Feishu bot webhook
        // In production, WebSocket mode is preferred but requires more complex handshake
        loop {
            sleep(Duration::from_secs(2)).await;
        }
    }

    async fn get_tenant_access_token(&self) -> Result<String> {
        let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
        let resp = self
            .http_client
            .post(url)
            .json(&json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .context("Failed to request tenant access token")?;

        let data: TenantAccessTokenResp = resp
            .json()
            .await
            .context("Failed to parse tenant access token response")?;

        if data.code != 0 {
            anyhow::bail!(
                "Feishu API error: {} - {}",
                data.code,
                data.msg.unwrap_or_default()
            );
        }

        Ok(data.tenant_access_token)
    }

    async fn send_text_message(&self, token: &str, chat_id: &str, text: &str) -> Result<()> {
        let url = "https://open.feishu.cn/open-apis/im/v1/messages";
        let resp = self
            .http_client
            .post(url)
            .query(&[("receive_id_type", "chat_id")])
            .header("Authorization", format!("Bearer {}", token))
            .json(&json!({
                "receive_id": chat_id,
                "content": json!({"text": text}).to_string(),
                "msg_type": "text",
            }))
            .send()
            .await
            .context("Failed to send Feishu message")?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Feishu send message failed: {} - {}", status, body);
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct TenantAccessTokenResp {
    code: i32,
    #[serde(default)]
    msg: Option<String>,
    #[serde(rename = "tenant_access_token")]
    tenant_access_token: String,
}
