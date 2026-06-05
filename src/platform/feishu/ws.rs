//! Feishu pbbp2 WebSocket transport: connect, heartbeat, read loop, frame ACK.

use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::time::{sleep, Duration as TokioDuration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message as WsMessage};
use tracing::{error, info, warn};

use crate::platform::proto::{Frame, Header};

use super::{FeishuPlatform, METHOD_CONTROL, METHOD_DATA, WsClientConfig};

type WsWrite = std::sync::Arc<
    tokio::sync::Mutex<
        futures::stream::SplitSink<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            WsMessage,
        >,
    >,
>;

pub(crate) fn build_ping_frame(service_id: i32) -> Frame {
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: METHOD_CONTROL,
        headers: vec![Header {
            key: "type".to_string(),
            value: "ping".to_string(),
        }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

pub(crate) fn build_ack_frame(original_frame: &Frame) -> Frame {
    let mut ack_frame = original_frame.clone();

    if !ack_frame.headers.iter().any(|h| h.key == "biz_rt") {
        ack_frame.headers.push(Header {
            key: "biz_rt".to_string(),
            value: "0".to_string(),
        });
    }

    let ack = serde_json::json!({
        "code": 200,
        "headers": {},
        "data": null
    });
    ack_frame.payload = Some(ack.to_string().into_bytes());
    ack_frame
}

fn service_id_from_url(ws_url: &str) -> i32 {
    ws_url
        .split('?')
        .nth(1)
        .and_then(|q| q.split('&').find(|p| p.starts_with("service_id=")))
        .and_then(|p| p.strip_prefix("service_id="))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

impl FeishuPlatform {
    pub(crate) async fn run_websocket(
        &self,
        ws_url: &str,
        client_config: WsClientConfig,
    ) -> Result<()> {
        info!("Connecting to Feishu WebSocket: {}", ws_url);
        crate::platform::status::set_state(
            "feishu",
            crate::platform::status::ConnectionState::Connecting,
        );

        let (ws_stream, _) = connect_async(ws_url)
            .await
            .context("Failed to connect to Feishu WebSocket")?;
        info!("Feishu WebSocket connected successfully");
        crate::platform::status::set_state(
            "feishu",
            crate::platform::status::ConnectionState::Connected,
        );

        let (write, mut read) = ws_stream.split();
        let write = std::sync::Arc::new(tokio::sync::Mutex::new(write));
        let service_id = service_id_from_url(ws_url);

        {
            let ping = build_ping_frame(service_id);
            let ping_bytes = crate::platform::proto::encode_frame(&ping);
            let mut w = write.lock().await;
            if w.send(WsMessage::Binary(ping_bytes.into())).await.is_ok() {
                info!("Sent initial PING for service_id={}", service_id);
            }
        }

        sleep(TokioDuration::from_millis(200)).await;

        let heartbeat_write = write.clone();
        let heartbeat_handle = tokio::spawn(async move {
            let interval = TokioDuration::from_secs(client_config.ping_interval.max(1) as u64);
            loop {
                sleep(interval).await;
                let ping = build_ping_frame(service_id);
                let ping_bytes = crate::platform::proto::encode_frame(&ping);
                let mut w = heartbeat_write.lock().await;
                if let Err(e) = w.send(WsMessage::Binary(ping_bytes.into())).await {
                    warn!("Feishu WebSocket heartbeat send error: {}", e);
                    break;
                }
            }
        });

        let platform = self.clone();
        let read_timeout_secs = client_config.ping_interval.max(1) as u64 * 3;
        let read_result: Result<()> = async {
            loop {
                match tokio::time::timeout(TokioDuration::from_secs(read_timeout_secs), read.next())
                    .await
                {
                    Ok(Some(Ok(WsMessage::Binary(data)))) => {
                        if let Some(frame) = crate::platform::proto::Frame::decode(&data) {
                            platform.handle_ws_frame(&frame, &write).await;
                        } else {
                            info!("Received invalid protobuf frame ({} bytes)", data.len());
                        }
                    }
                    Ok(Some(Ok(WsMessage::Ping(data)))) => {
                        let mut w = write.lock().await;
                        w.send(WsMessage::Pong(data)).await.ok();
                    }
                    Ok(Some(Ok(WsMessage::Close(_)))) => {
                        info!("Feishu WebSocket connection closed by server");
                        break;
                    }
                    Ok(Some(Ok(WsMessage::Text(text)))) => {
                        info!("Unexpected text frame: {}", text);
                    }
                    Ok(Some(Ok(_))) => {}
                    Ok(Some(Err(e))) => {
                        error!("Feishu WebSocket read error: {}", e);
                        return Err(anyhow::anyhow!("Feishu WebSocket read error: {}", e));
                    }
                    Ok(None) => {
                        info!("Feishu WebSocket stream ended");
                        break;
                    }
                    Err(_) => {
                        warn!("Feishu WebSocket read timeout after {}s", read_timeout_secs);
                        return Err(anyhow::anyhow!("Feishu WebSocket read timeout"));
                    }
                }
            }
            info!("Feishu WebSocket read loop ended");
            Ok(())
        }
        .await;

        heartbeat_handle.abort();
        crate::platform::status::set_state(
            "feishu",
            crate::platform::status::ConnectionState::Disconnected,
        );
        read_result
    }

    async fn handle_ws_frame(&self, frame: &Frame, write: &WsWrite) {
        info!(
            "Feishu frame: method={} service={} payload_len={:?} headers={:?}",
            frame.method,
            frame.service,
            frame.payload.as_ref().map(|p| p.len()),
            frame
                .headers
                .iter()
                .map(|h| format!("{}={}", h.key, h.value))
                .collect::<Vec<_>>(),
        );

        match frame.method {
            METHOD_CONTROL => {
                info!(
                    "Feishu control frame type={:?}",
                    frame
                        .headers
                        .iter()
                        .find(|h| h.key == "type")
                        .map(|h| &h.value)
                );
                if let Some(ref payload) = frame.payload {
                    if let Ok(cfg) = serde_json::from_slice::<serde_json::Value>(payload) {
                        info!("Feishu control frame payload: {}", cfg);
                    }
                }
            }
            METHOD_DATA => {
                let ack = build_ack_frame(frame);
                let ack_bytes = crate::platform::proto::encode_frame(&ack);
                let mut w = write.lock().await;
                if let Err(e) = w.send(WsMessage::Binary(ack_bytes.into())).await {
                    warn!("Failed to send ACK: {}", e);
                }
                drop(w);

                if let Some(payload) = frame.payload.clone() {
                    let platform = self.clone();
                    tokio::spawn(async move {
                        platform.dispatch_ws_data_payload(payload).await;
                    });
                }
            }
            _ => {
                info!("Feishu unhandled frame method={}", frame.method);
            }
        }
    }
}
