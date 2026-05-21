use anyhow::Result;
use async_trait::async_trait;

pub mod feishu;
pub mod telegram;
pub mod proto;

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
