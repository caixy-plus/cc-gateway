use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::warn;

use crate::platform::feishu::FeishuConfig;

/// Shared token manager used by both `FeishuPlatform` and `FeishuAuthMiddleware`.
#[derive(Clone)]
pub struct TokenManager {
    config: FeishuConfig,
    pub(crate) cached_token: Arc<RwLock<Option<String>>>,
    pub(crate) token_fetched_at: Arc<RwLock<Option<Instant>>>,
    http_client: reqwest::Client,
}

impl TokenManager {
    pub fn new(config: FeishuConfig) -> Self {
        Self {
            config,
            cached_token: Arc::new(RwLock::new(None)),
            token_fetched_at: Arc::new(RwLock::new(None)),
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn get_tenant_access_token(&self) -> Result<String> {
        {
            let cached = self.cached_token.read().await;
            let fetched_at = self.token_fetched_at.read().await;
            if let (Some(token), Some(instant)) = (cached.as_ref(), fetched_at.as_ref()) {
                if instant.elapsed().as_secs() < 3300 {
                    return Ok(token.clone());
                }
            }
        }

        let token = self.fetch_tenant_access_token().await?;
        let mut cached = self.cached_token.write().await;
        let mut fetched_at = self.token_fetched_at.write().await;
        *cached = Some(token.clone());
        *fetched_at = Some(Instant::now());
        Ok(token)
    }

    pub async fn refresh_token(&self) -> Result<String> {
        let token = self.fetch_tenant_access_token().await?;
        let mut cached = self.cached_token.write().await;
        let mut fetched_at = self.token_fetched_at.write().await;
        *cached = Some(token.clone());
        *fetched_at = Some(Instant::now());
        Ok(token)
    }

    pub async fn invalidate_token_cache(&self) {
        let mut cached = self.cached_token.write().await;
        let mut fetched_at = self.token_fetched_at.write().await;
        *cached = None;
        *fetched_at = None;
    }

    pub fn is_auth_error(e: &anyhow::Error) -> bool {
        let s = e.to_string();
        s.contains("99991663")
            || s.contains("99991661")
            || s.contains("99991664")
            || s.contains("Invalid access token")
    }

    async fn fetch_tenant_access_token(&self) -> Result<String> {
        let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
        let resp = self
            .http_client
            .post(url)
            .json(&serde_json::json!({
                "app_id": self.config.app_id,
                "app_secret": self.config.app_secret,
            }))
            .send()
            .await
            .context("Failed to request tenant access token")?;

        let data: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse tenant access token response")?;

        let code = data.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = data
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            anyhow::bail!("Feishu API error: {} - {}", code, msg);
        }

        let token = data
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing tenant_access_token in response"))?;
        Ok(token.to_string())
    }
}

/// Reqwest middleware that injects Feishu tenant access tokens and refreshes
/// them automatically when Feishu returns an auth error (99991663 etc.).
#[derive(Clone)]
pub struct FeishuAuthMiddleware {
    token_manager: TokenManager,
}

impl FeishuAuthMiddleware {
    pub fn new(token_manager: TokenManager) -> Self {
        Self { token_manager }
    }
}

#[async_trait]
impl Middleware for FeishuAuthMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        _extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        // Inject current token.
        let token = self
            .token_manager
            .get_tenant_access_token()
            .await
            .map_err(|e| reqwest_middleware::Error::Middleware(e.into()))?;

        req.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token))
                .map_err(|e| reqwest_middleware::Error::Middleware(e.into()))?,
        );

        let resp = next.run(req, _extensions).await?;

        // If Feishu returned an HTTP auth error, invalidate the cache so the
        // next request gets a fresh token.  Callers may retry once if they
        // also need to handle JSON-level error codes (e.g. 99991663).
        if Self::is_feishu_auth_error(&resp) {
            warn!("[Feishu] HTTP auth error detected, invalidating token cache");
            self.token_manager.invalidate_token_cache().await;
        }

        Ok(resp)
    }
}

impl FeishuAuthMiddleware {
    /// Returns true if the response indicates an invalid/expired token.
    /// This only checks HTTP status (401/403) without consuming the body;
    /// JSON code 99991663 is returned with HTTP 400, so callers should still
    /// handle that in their business logic if they need precise retry.
    fn is_feishu_auth_error(resp: &Response) -> bool {
        let status = resp.status();
        status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN
    }
}
