//! QQ Bot Gateway WebSocket (opcode 10/2/1/0/6).

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use super::api::{QqApiClient, INTENTS_GROUP_AND_C2C};

#[derive(Clone)]
pub struct GatewaySession {
    pub session_id: String,
    pub last_seq: Option<u64>,
}

#[allow(dead_code)]
pub enum GatewayEvent {
    Ready(GatewaySession),
    Dispatch {
        event_type: String,
        data: Value,
        #[allow(dead_code)]
        seq: Option<u64>,
    },
    ReconnectRequested,
    InvalidSession,
}

pub async fn run_gateway(
    api: QqApiClient,
    event_tx: mpsc::UnboundedSender<GatewayEvent>,
) -> Result<()> {
    let mut resume: Option<GatewaySession> = None;

    loop {
        let gateway = api.fetch_gateway().await?;
        info!(
            "[QQ] Connecting to gateway (shards={}, sandbox={})",
            gateway.shards.max(1),
            api.sandbox
        );

        let (ws_stream, _) = connect_async(&gateway.url)
            .await
            .with_context(|| format!("QQ websocket connect failed: {}", gateway.url))?;
        let (mut write, mut read) = ws_stream.split();

        let mut heartbeat_interval_ms = 45_000u64;
        let mut identified = false;
        let last_seq = Arc::new(AtomicU64::new(
            resume.as_ref().and_then(|s| s.last_seq).unwrap_or(0),
        ));
        let mut heartbeat = tokio::time::interval(Duration::from_millis(heartbeat_interval_ms));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                incoming = read.next() => {
                    let Some(msg) = incoming else {
                        warn!("[QQ] Gateway stream ended");
                        break;
                    };
                    let msg = msg.context("QQ websocket read error")?;
                    let text = match msg {
                        Message::Text(t) => t,
                        Message::Binary(b) => String::from_utf8_lossy(&b).into_owned().into(),
                        Message::Close(_) => {
                            warn!("[QQ] Gateway closed connection");
                            break;
                        }
                        Message::Ping(_) | Message::Pong(_) => continue,
                        _ => continue,
                    };

                    let payload: Value = serde_json::from_str(&text)
                        .context("QQ gateway invalid JSON frame")?;
                    let op = payload.get("op").and_then(|v| v.as_u64()).unwrap_or(999);
                    let seq = payload.get("s").and_then(|v| v.as_u64());
                    if let Some(s) = seq {
                        last_seq.store(s, Ordering::Relaxed);
                    }

                    match op {
                        10 => {
                            heartbeat_interval_ms = payload
                                .get("d")
                                .and_then(|d| d.get("heartbeat_interval"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(45_000);
                            heartbeat = tokio::time::interval(Duration::from_millis(heartbeat_interval_ms));
                            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

                            let token = api.access_token().await?;
                            if let Some(ref sess) = resume {
                                let resume_payload = json!({
                                    "op": 6,
                                    "d": {
                                        "token": format!("QQBot {}", token),
                                        "session_id": sess.session_id,
                                        "seq": last_seq.load(Ordering::Relaxed),
                                    }
                                });
                                write.send(Message::Text(resume_payload.to_string().into())).await?;
                            } else {
                                let identify = json!({
                                    "op": 2,
                                    "d": {
                                        "token": format!("QQBot {}", token),
                                        "intents": INTENTS_GROUP_AND_C2C,
                                        "shard": [0, 1],
                                        "properties": {
                                            "$os": "linux",
                                            "$browser": "cc-gateway",
                                            "$device": "cc-gateway"
                                        }
                                    }
                                });
                                write.send(Message::Text(identify.to_string().into())).await?;
                            }
                            identified = true;
                        }
                        0 => {
                            let event_type = payload
                                .get("t")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let data = payload.get("d").cloned().unwrap_or(json!({}));
                            if event_type == "READY" {
                                let session_id = data
                                    .get("session_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let session = GatewaySession {
                                    session_id: session_id.clone(),
                                    last_seq: seq,
                                };
                                resume = Some(session.clone());
                                let _ = event_tx.send(GatewayEvent::Ready(session));
                                info!("[QQ] Gateway READY session_id={}", session_id);
                            } else if event_type == "RESUMED" {
                                debug!("[QQ] Gateway RESUMED");
                            } else if !event_type.is_empty() {
                                let _ = event_tx.send(GatewayEvent::Dispatch {
                                    event_type,
                                    data,
                                    seq,
                                });
                            }
                        }
                        7 => {
                            warn!("[QQ] Gateway requested reconnect");
                            let _ = event_tx.send(GatewayEvent::ReconnectRequested);
                            break;
                        }
                        9 => {
                            warn!("[QQ] Invalid session — re-identifying");
                            resume = None;
                            let _ = event_tx.send(GatewayEvent::InvalidSession);
                            break;
                        }
                        11 => {}
                        _ => debug!("[QQ] Ignoring gateway op={}", op),
                    }
                }
                _ = heartbeat.tick(), if identified => {
                    let hb = json!({
                        "op": 1,
                        "d": last_seq.load(Ordering::Relaxed)
                    });
                    if write.send(Message::Text(hb.to_string().into())).await.is_err() {
                        break;
                    }
                }
            }
        }

        if !identified {
            anyhow::bail!("QQ gateway closed before identify completed");
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
        error!("[QQ] Reconnecting gateway…");
    }
}
