use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::claude::controller::ClaudeController;
use crate::command::router::CommandRouter;
use crate::config::model::GatewayConfig;
use crate::platform::feishu::FeishuPlatform;

pub struct DaemonEngine {
    config: GatewayConfig,
}

impl DaemonEngine {
    pub fn new(config: GatewayConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let controller = Arc::new(Mutex::new(ClaudeController::new(
            self.config.claude.clone(),
        )));
        {
            let ctrl = controller.lock().await;
            let cwd = std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| {
                    dirs::home_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| "/".to_string())
                });
            ctrl.init_work_dir(cwd).await;
        }
        let default_dir = &self.config.default_dir;
        let router = Arc::new(CommandRouter::new(controller.clone(), default_dir));

        // Start Feishu platform if enabled
        let mut feishu_handle = None;
        if self.config.feishu.enabled {
            let platform = FeishuPlatform::new(
                self.config.feishu.clone(),
                &self.config.default_dir,
                router.clone(),
                controller.clone(),
            );
            if self.config.feishu.mode == "webhook" {
                feishu_handle = Some(tokio::spawn(async move {
                    if let Err(e) = platform.run_webhook().await {
                        error!("Feishu webhook server error: {}", e);
                    }
                }));
            } else {
                feishu_handle = Some(tokio::spawn(async move {
                    if let Err(e) = platform.run().await {
                        error!("Feishu platform error: {}", e);
                    }
                }));
            }
        }

        info!("cc-gateway daemon is running");

        // Wait for shutdown signal
        self.wait_shutdown_signal().await?;

        // Cleanup
        {
            let ctrl = controller.lock().await;
            let _ = ctrl.stop_session().await;
        }

        if let Some(handle) = feishu_handle {
            handle.abort();
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
