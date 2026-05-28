use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{debug, info, warn};

use super::{build_http_response, FeishuPlatform};

impl FeishuPlatform {
    /// Verify a Feishu event subscription challenge.
    /// When configuring a webhook event subscription, Feishu sends a "challenge"
    /// field that must be decrypted and returned.
    pub fn verify_challenge(&self, body: &Value) -> Result<Value> {
        let challenge = body.get("challenge").and_then(|v| v.as_str()).unwrap_or("");

        if challenge.is_empty() {
            return Ok(json!({"challenge": ""}));
        }

        if self.config.encrypt_key.is_empty() {
            warn!("No encrypt_key configured; echoing challenge without decryption");
            return Ok(json!({"challenge": challenge}));
        }

        // TODO: Implement proper AES-256-CBC decryption of the challenge
        // using the configured encrypt_key.
        warn!("Challenge verification with encryption not yet implemented");
        Ok(json!({"challenge": challenge}))
    }

    /// Handle a webhook event delivered via HTTP POST to /webhook.
    pub async fn handle_webhook_event(&self, body: &Value) -> Result<()> {
        let header = match body.get("header") {
            Some(h) => h,
            None => {
                warn!("Webhook event missing header");
                return Ok(());
            }
        };

        let event_type = match header.get("event_type").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => {
                warn!("Webhook event missing event_type");
                return Ok(());
            }
        };

        match event_type {
            "im.message.receive_v1" => {
                if let Some(normalized) = self.normalize_im_event(body) {
                    if !self.dedup_cache.contains(&normalized.message_id) {
                        self.dedup_cache.insert(normalized.message_id.clone());
                        self.process_normalized_message(normalized).await;
                    }
                }
            }
            "card.action.trigger" => {
                self.handle_card_action(body).await;
            }
            other => {
                warn!("Unhandled webhook event type: {}", other);
            }
        }

        Ok(())
    }

    /// Start a minimal HTTP webhook server (no extra framework deps).
    pub async fn run_webhook(&self) -> Result<()> {
        let bind_addr = &self.config.webhook_bind;
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .with_context(|| format!("Failed to bind webhook server to {}", bind_addr))?;
        info!("Feishu webhook server listening on {}", bind_addr);

        loop {
            let (mut stream, addr) = listener
                .accept()
                .await
                .context("Failed to accept webhook connection")?;
            let platform = self.clone();
            let ip = addr.ip().to_string();
            if !platform.rate_limiter.check(&ip) {
                warn!("[Feishu] Webhook rate limit exceeded for {}", ip);
                platform.anomaly_tracker.record(&ip, 429);
                let _ = {
                    use tokio::io::AsyncWriteExt;
                    stream
                        .write_all(
                            build_http_response(429, r#"{"code":1,"msg":"too many requests"}"#)
                                .as_bytes(),
                        )
                        .await
                };
                continue;
            }
            tokio::spawn(async move {
                if let Err(e) = platform.handle_webhook_connection(stream, addr).await {
                    debug!("Webhook connection from {} error: {}", addr, e);
                }
            });
        }
    }

    async fn handle_webhook_connection(
        &self,
        stream: tokio::net::TcpStream,
        addr: std::net::SocketAddr,
    ) -> Result<()> {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut request_line = String::new();

        if reader.read_line(&mut request_line).await? == 0 {
            return Ok(());
        }
        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            return Ok(());
        }
        let method = parts[0].to_uppercase();
        let path = parts[1].to_string();

        let mut headers = std::collections::HashMap::new();
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            if line.trim().is_empty() {
                break;
            }
            if let Some((k, v)) = line.trim().split_once(": ") {
                headers.insert(k.to_lowercase(), v.to_string());
            }
        }

        let content_length = headers
            .get("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        // We require Content-Length for /webhook POST requests.
        // This keeps the implementation simple and ensures our size limit can't be bypassed
        // via chunked transfer encoding.
        if method == "POST" && path == "/webhook" && !headers.contains_key("content-length") {
            warn!(
                "[Feishu] Webhook request from {} rejected: missing Content-Length",
                addr
            );
            let _ = writer
                .write_all(
                    build_http_response(411, r#"{"code":1,"msg":"length required"}"#).as_bytes(),
                )
                .await;
            return Ok(());
        }
        const MAX_WEBHOOK_BODY_BYTES: usize = 1024 * 1024; // 1 MB
        if content_length > MAX_WEBHOOK_BODY_BYTES {
            warn!(
                "[Feishu] Webhook request from {} rejected: Content-Length {} exceeds limit",
                addr, content_length
            );
            let _ = writer
                .write_all(
                    build_http_response(413, r#"{"code":1,"msg":"request body too large"}"#)
                        .as_bytes(),
                )
                .await;
            return Ok(());
        }
        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            if let Err(e) = reader.read_exact(&mut body).await {
                warn!(
                    "[Feishu] Webhook request from {} read body failed (len={}): {}",
                    addr, content_length, e
                );
                let _ = writer
                    .write_all(
                        build_http_response(400, r#"{"code":1,"msg":"bad request"}"#).as_bytes(),
                    )
                    .await;
                return Ok(());
            }
        }

        debug!(
            "Webhook {} {} from {} (body={} bytes)",
            method, path, addr, content_length
        );

        let ip = addr.ip().to_string();
        let (status, body_str) = match (method.as_str(), path.as_str()) {
            ("POST", "/webhook") => match serde_json::from_slice::<Value>(&body) {
                Ok(json) => {
                    if json.get("challenge").is_some() {
                        match self.verify_challenge(&json) {
                            Ok(resp) => (200, resp.to_string()),
                            Err(e) => (400, json!({"error": e.to_string()}).to_string()),
                        }
                    } else {
                        let event_type = json
                            .get("header")
                            .and_then(|h| h.get("event_type"))
                            .and_then(|v| v.as_str());
                        match event_type {
                            Some(_) => {
                                let _ = self.handle_webhook_event(&json).await;
                                (200, r#"{"code":0}"#.to_string())
                            }
                            None => {
                                warn!("Webhook event missing event_type");
                                (200, r#"{"code":0}"#.to_string())
                            }
                        }
                    }
                }
                Err(_) => (400, r#"{"code":1,"msg":"invalid json"}"#.to_string()),
            },
            _ => (404, r#"{"code":1,"msg":"not found"}"#.to_string()),
        };

        self.anomaly_tracker.record(&ip, status);
        writer
            .write_all(build_http_response(status, &body_str).as_bytes())
            .await?;
        Ok(())
    }
}
