use anyhow::{Context, Result};
use reqwest::{Request, Response};
use reqwest_middleware::{Middleware, Next};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::config::model::FeishuConfig;

/// Manages Feishu tenant access token with caching and auto-refresh.
#[derive(Clone)]
pub struct TokenManager {
    config: FeishuConfig,
    cached_token: Arc<RwLock<Option<(String, std::time::Instant)>>>,
    token_ttl: std::time::Duration,
}

impl TokenManager {
    pub fn new(config: FeishuConfig) -> Self {
        Self {
            config,
            cached_token: Arc::new(RwLock::new(None)),
            token_ttl: std::time::Duration::from_secs(3600), // 1 hour default
        }
    }

    /// Get a valid tenant access token, refreshing if necessary.
    pub async fn get_tenant_access_token(&self) -> Result<String> {
        // Check cache first
        {
            let cache = self.cached_token.read().await;
            if let Some((ref token, ref created)) = *cache {
                if created.elapsed() < self.token_ttl {
                    return Ok(token.clone());
                }
            }
        }

        // Refresh
        let url = "https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal";
        let client = reqwest::Client::new();
        let resp = client
            .post(url)
            .json(&serde_json::json!({
                "app_id": &self.config.app_id,
                "app_secret": &self.config.app_secret,
            }))
            .send()
            .await
            .context("Failed to request tenant access token")?;

        let body: serde_json::Value = resp
            .json()
            .await
            .context("Failed to parse tenant access token response")?;

        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
        if code != 0 {
            let msg = body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Feishu tenant access token error: {} - {}", code, msg);
        }

        let token = body
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .context("Missing tenant_access_token in response")?
            .to_string();

        debug!("Refreshed Feishu tenant access token");
        let mut cache = self.cached_token.write().await;
        *cache = Some((token.clone(), std::time::Instant::now()));

        Ok(token)
    }

    /// Invalidate the cached token (e.g., after a 401 response).
    pub async fn invalidate_token_cache(&self) {
        let mut cache = self.cached_token.write().await;
        *cache = None;
        debug!("Invalidated Feishu tenant access token cache");
    }

    /// Check if an error is an authentication error that should trigger token refresh.
    pub fn is_auth_error(err: &anyhow::Error) -> bool {
        let msg = format!("{}", err);
        msg.contains("99991663")  // tenant access token invalid
            || msg.contains("99991664")  // tenant access token expired
            || msg.contains("401")
    }
}

/// reqwest-middleware that auto-injects the Feishu tenant access token
/// into the Authorization header of every request.
#[derive(Clone)]
pub struct FeishuAuthMiddleware {
    token_manager: TokenManager,
}

impl FeishuAuthMiddleware {
    pub fn new(token_manager: TokenManager) -> Self {
        Self { token_manager }
    }
}

#[async_trait::async_trait]
impl Middleware for FeishuAuthMiddleware {
    async fn handle(
        &self,
        mut req: Request,
        extensions: &mut http::Extensions,
        next: Next<'_>,
    ) -> reqwest_middleware::Result<Response> {
        // Skip token injection for auth endpoint itself
        let skip_auth = req.url().path().contains("auth/v3/tenant_access_token");

        if !skip_auth {
            match self.token_manager.get_tenant_access_token().await {
                Ok(token) => {
                    let header_val = format!("Bearer {}", token);
                    req.headers_mut()
                        .insert("Authorization", header_val.parse().unwrap());
                }
                Err(e) => {
                    warn!("Failed to get tenant access token for request: {}", e);
                }
            }
        }

        next.run(req, extensions).await
    }
}
