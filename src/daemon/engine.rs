use anyhow::Result;
use tracing::{error, info};

use crate::config::model::GatewayConfig;
use crate::platform::Platform;
use crate::platform::feishu::FeishuPlatform;
use crate::platform::telegram::TelegramPlatform;

pub struct DaemonEngine {
    config: GatewayConfig,
}

impl DaemonEngine {
    pub fn new(config: GatewayConfig) -> Self {
        Self { config }
    }

    pub async fn run(self, listener: tokio::net::TcpListener) -> Result<()> {
        let log_path = shellexpand::tilde(&self.config.log.file).to_string();
        crate::daemon::cleaner::start_background_task(
            log_path,
            self.config.log.max_lines,
            self.config.log.max_size_mb,
            self.config.media_retention_days,
        );

        // Start history recorder for WebUI sessions
        crate::history::recorder::start_recorder();

        // Initialize SQLite database and restore persisted sessions
        if let Err(e) = crate::db::init_schema() {
            error!("Failed to initialize session database: {}", e);
        }
        crate::session::manager::GLOBAL_SESSIONS.load_sessions();

        // Start all enabled platforms concurrently
        let mut platforms: Vec<(Box<dyn Platform>, tokio::task::JoinHandle<()>)> = Vec::new();

        if self.config.feishu.enabled {
            let platform = FeishuPlatform::new(
                self.config.feishu.clone(),
                &self.config.default_dir,
                self.config.claude.clone(),
                self.config.show_thinking,
            );
            let platform_for_spawn = platform.clone();
            let handle = tokio::spawn(async move {
                if let Err(e) = platform_for_spawn.run().await {
                    error!("Feishu platform error: {}", e);
                }
            });
            platforms.push((Box::new(platform), handle));
        }

        if self.config.telegram.enabled {
            let platform = TelegramPlatform::new(
                self.config.telegram.clone(),
                &self.config.default_dir,
                self.config.claude.clone(),
                self.config.show_thinking,
            );
            let platform_for_spawn = platform.clone();
            let handle = tokio::spawn(async move {
                if let Err(e) = platform_for_spawn.run().await {
                    error!("Telegram platform error: {}", e);
                }
            });
            platforms.push((Box::new(platform), handle));
        }

        // Start HTTP server on the singleton port
        let app = crate::web::server::create_app(&self.config);
        let server_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("HTTP server error: {}", e);
            }
        });
        info!("HTTP server listening on http://127.0.0.1:{}", self.config.port);

        if platforms.is_empty() {
            info!("No platform enabled. Daemon is idle.");
        } else {
            info!("cc-gateway daemon is running ({} platform(s))", platforms.len());
        }

        // Wait for shutdown signal
        self.wait_shutdown_signal().await?;

        // Graceful shutdown: stop HTTP server
        info!("Shutting down HTTP server...");
        server_handle.abort();

        // Graceful shutdown: notify all platforms and their chat sessions
        if !platforms.is_empty() {
            info!("Shutting down all chat sessions...");
            for (platform, handle) in platforms {
                platform.shutdown().await;
                handle.abort();
            }
        }

        info!("cc-gateway daemon stopped");
        Ok(())
    }

    #[cfg(unix)]
    async fn wait_shutdown_signal(&self) -> Result<()> {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down...");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down...");
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    async fn wait_shutdown_signal(&self) -> Result<()> {
        let mut ctrl_c = tokio::signal::windows::ctrl_c()?;
        let mut ctrl_break = tokio::signal::windows::ctrl_break()?;

        tokio::select! {
            _ = ctrl_c.recv() => {
                info!("Received Ctrl+C, shutting down...");
            }
            _ = ctrl_break.recv() => {
                info!("Received Ctrl+Break, shutting down...");
            }
        }
        Ok(())
    }
}
