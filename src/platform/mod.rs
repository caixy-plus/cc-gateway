use anyhow::Result;
use async_trait::async_trait;

pub mod feishu;
pub mod inbound_media;
pub mod proto;
pub mod telegram;

/// Platform abstraction for chat bot integrations (Feishu, Telegram, Discord, etc.).
/// Each platform manages its own connection lifecycle and message handling,
/// while delegating command routing and Claude session management to shared
/// components (`CommandRouter` and `ChatSession`).
#[async_trait]
pub trait Platform: Send + Sync {
    /// Start the platform's event loop (WebSocket, webhook server, polling, etc.).
    /// This blocks until the platform is explicitly shut down or encounters a fatal error.
    async fn run(&self) -> Result<()>;

    /// Gracefully shut down the platform and all active chat sessions.
    async fn shutdown(&self);
}

/// Spawn a background task that listens to `DELIVER_BUS` and forwards
/// file-delivery requests to the platform-specific sender.
///
/// The `sender` is a synchronous callback that should internally spawn any
/// async work (e.g. `tokio::spawn`) so that the listener loop never blocks.
pub fn spawn_deliver_listener<F>(
    platform_name: &'static str,
    sender: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(String, String) + Send + Sync + 'static,
{
    tokio::spawn(async move {
        let mut rx = crate::web::state::DELIVER_BUS.subscribe();
        loop {
            match rx.recv().await {
                Ok(req) => {
                    // Try lookup by ChannelSession.id (UUID) first, then by platform chat_id.
                    let channel = crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                        .get_channel(&req.session_id)
                        .or_else(|| {
                            crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS
                                .list_channels()
                                .into_iter()
                                .find(|c| {
                                    c.platform == platform_name && c.channel_id == req.session_id
                                })
                        });
                    if let Some(channel) = channel {
                        let text = if let Some(ref msg) = req.message {
                            format!("{}\n📎 {}", msg, req.path)
                        } else {
                            format!("📎 {}", req.path)
                        };
                        sender(channel.channel_id, text);
                    }
                }
                Err(_) => continue,
            }
        }
    })
}
