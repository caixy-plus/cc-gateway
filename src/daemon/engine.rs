use std::sync::Arc;

use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::Notify;
use tracing::{error, info};

use crate::config::model::GatewayConfig;
use crate::platform::feishu::FeishuPlatform;
use crate::platform::telegram::TelegramPlatform;
use crate::platform::Platform;

pub struct DaemonEngine {
    config: GatewayConfig,
    config_path: Option<PathBuf>,
}

impl DaemonEngine {
    #[allow(dead_code)]
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            config_path: None,
        }
    }

    pub fn new_with_config_path(config: GatewayConfig, config_path: Option<PathBuf>) -> Self {
        Self {
            config,
            config_path,
        }
    }

    pub async fn run(self, listener: tokio::net::TcpListener) -> Result<()> {
        let log_path = shellexpand::tilde(&self.config.log.file).to_string();
        crate::daemon::cleaner::start_background_task(
            log_path,
            self.config.log.max_lines,
            self.config.log.max_size_mb,
            self.config.media_retention_days,
            crate::config::model::effective_session_retention_per_channel(
                self.config.session_retention_per_channel,
            ),
        );

        // Start history recorder for WebUI sessions
        crate::history::recorder::start_recorder();

        // Initialize SQLite database and restore persisted sessions
        if let Err(e) = crate::db::init_schema() {
            error!("Failed to initialize session database: {}", e);
        }
        crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS.load_from_db();

        // Start all enabled platforms concurrently
        let mut platforms: Vec<(Box<dyn Platform>, tokio::task::JoinHandle<()>)> = Vec::new();

        if self.config.feishu.enabled {
            let platform = FeishuPlatform::new(
                self.config.feishu.clone(),
                &self.config.default_dir,
                self.config.effective_agent_settings(),
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
                self.config.effective_agent_settings(),
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

        // Shutdown signal: used when a critical component (HTTP server) fails,
        // so the daemon exits cleanly instead of becoming a zombie.
        let shutdown_notify = Arc::new(Notify::new());

        // Start HTTP server on the singleton port
        let app =
            crate::web::server::create_app_with_config_path(&self.config, self.config_path.clone());
        let shutdown_for_server = shutdown_notify.clone();
        let server_handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                error!("HTTP server error: {}", e);
                shutdown_for_server.notify_one();
            }
        });
        info!(
            "HTTP server listening on http://127.0.0.1:{}",
            self.config.port
        );

        if platforms.is_empty() {
            info!("No platform enabled. Daemon is idle.");
        } else {
            info!(
                "cc-gateway daemon is running ({} platform(s))",
                platforms.len()
            );
        }

        // Wait for shutdown signal (OS signal or HTTP server failure)
        self.wait_shutdown_signal(shutdown_notify).await?;

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
    async fn wait_shutdown_signal(&self, shutdown_notify: Arc<Notify>) -> Result<()> {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;

        tokio::select! {
            _ = sigterm.recv() => {
                info!("Received SIGTERM, shutting down...");
            }
            _ = sigint.recv() => {
                info!("Received SIGINT, shutting down...");
            }
            _ = shutdown_notify.notified() => {
                info!("Internal shutdown triggered (HTTP server failure), shutting down...");
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    async fn wait_shutdown_signal(&self, shutdown_notify: Arc<Notify>) -> Result<()> {
        // On Windows, when the daemon is spawned with DETACHED_PROCESS, there is
        // no console and SetConsoleCtrlHandler fails.  Fall back to listening
        // only for the internal shutdown signal (triggered by HTTP server failure
        // or the shutdown API).
        let ctrl_c = tokio::signal::windows::ctrl_c().ok();
        let ctrl_break = tokio::signal::windows::ctrl_break().ok();

        if ctrl_c.is_none() && ctrl_break.is_none() {
            info!("No console attached – shutdown only via stop command or HTTP API");
            shutdown_notify.notified().await;
            info!("Internal shutdown triggered, shutting down...");
            return Ok(());
        }

        tokio::select! {
            _ = async {
                if let Some(mut c) = ctrl_c {
                    let _ = c.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("Received Ctrl+C, shutting down...");
            }
            _ = async {
                if let Some(mut b) = ctrl_break {
                    let _ = b.recv().await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => {
                info!("Received Ctrl+Break, shutting down...");
            }
            _ = shutdown_notify.notified() => {
                info!("Internal shutdown triggered (HTTP server failure), shutting down...");
            }
        }
        Ok(())
    }
}
