use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{debug, warn};

use super::{FeishuPlatform, NormalizedMessage};

impl FeishuPlatform {
    // -----------------------------------------------------------------------
    // Webhook challenge & event handling
    // -----------------------------------------------------------------------

    pub(crate) fn verify_challenge(&self, body: &Value) -> Result<Value> {
        let challenge = body
            .get("challenge")
            .and_then(|v| v.as_str())
            .context("Missing challenge field")?;
        Ok(json!({ "challenge": challenge }))
    }

    pub(crate) async fn handle_webhook_event(&self, body: &Value) -> Result<Option<NormalizedMessage>> {
        if body.get("challenge").is_some() {
            anyhow::bail!("Challenge requests should be handled by verify_challenge");
        }

        let event_type = body
            .get("header")
            .and_then(|h| h.get("event_type"))
            .and_then(|v| v.as_str());

        match event_type {
            Some("im.message.receive_v1") => {
                let normalized = self.normalize_message(body);
                Ok(normalized)
            }
            Some(other) => {
                debug!("Unhandled webhook event type: {}", other);
                Ok(None)
            }
            None => {
                warn!("Webhook event missing event_type");
                Ok(None)
            }
        }
    }
}
