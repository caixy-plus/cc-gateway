//! Main daemon engine ([`DaemonEngine`]).
//!
//! Spawned by the `cc-gateway _daemon` subcommand ([`crate::daemon::run`]), it is responsible for:
//!
//! 1. Starting background tasks such as the log cleaner and history recorder;
//! 2. Initializing the SQLite database and restoring channels, sessions, and pairing states from disk;
//! 3. Starting the chat platforms enabled by the user (Feishu, Telegram);
//! 4. Starting the HTTP and WebUI server (binding to a singleton port to ensure only one daemon instance runs);
//! 5. Waiting for `SIGTERM`, `SIGINT`, or a "critical component crash" signal to gracefully shut down all platforms.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::Notify;
use tracing::{error, info};

use crate::config::model::GatewayConfig;
use crate::config::platform_registry;

/// Main daemon engine struct.
///
/// The engine itself only holds the configuration and configuration file path. All runtime states
/// (channels, sessions, and pairings) are stored in global singletons ([`crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS`]
/// and [`crate::session::pairing::GLOBAL_PAIRING_MANAGER`]) to facilitate cross-platform and HTTP access.
pub struct DaemonEngine {
    config: GatewayConfig,
    /// Configuration file path (passed to the WebUI to allow the user to edit it directly in Settings).
    config_path: Option<PathBuf>,
}

impl DaemonEngine {
    /// Constructor without a `config_path` (preserved for tests and legacy callers).
    #[allow(dead_code)]
    pub fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            config_path: None,
        }
    }

    /// Constructor for production: includes `config_path` to facilitate WebUI read/write operations on the same file.
    pub fn new_with_config_path(config: GatewayConfig, config_path: Option<PathBuf>) -> Self {
        Self {
            config,
            config_path,
        }
    }

    /// Starts the daemon engine.
    ///
    /// The `listener` is the singleton TCP port pre-bound by [`super::daemon::run`] to guarantee
    /// that only one daemon runs on the machine (binding will fail if the port is already in use).
    pub async fn run(self, listener: tokio::net::TcpListener) -> Result<()> {
        // 1. Start the background log cleaner: rotates logs based on `log.max_lines` and `log.max_size_mb`,
        //    and clears old sessions and attachments according to `media_retention_days` and `session_retention_per_channel`.
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

        // 2. Start the WebUI session history recorder (subscribes to `EVENT_BUS` and writes events to
        //    `~/.cc-gateway/history/{session_id}.jsonl`).
        crate::history::recorder::start_recorder();

        // 3. Initialize the SQLite database and restore previously persisted channel, session,
        //    and pairing states from disk. This ensures the session list remains visible in the WebUI after a daemon restart.
        if let Err(e) = crate::db::init_schema() {
            error!("Failed to initialize session database: {}", e);
        }
        crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS.load_from_db();
        crate::session::pairing::GLOBAL_PAIRING_MANAGER.load_from_db();
        // Synchronize the `require_pairing` flag from the config to the global pairing manager.
        platform_registry::apply_pairing_flags_from_config(&self.config);

        // 4. Start the WebUI file delivery listener (used to deliver "cards/keyboards" for directory listings like `/api/cmd/ll`).
        crate::web::files::spawn_webui_deliver_listener();

        // 5. Start all enabled chat platforms (concurrently).
        let platforms =
            platform_registry::start_enabled_platforms(&self.config).unwrap_or_else(|e| {
                error!("Failed to start platforms: {e}");
                Vec::new()
            });

        // 6. Shutdown notification signal in case a critical component crashes (e.g., if the HTTP server port is
        //    preempted, the daemon should not become a zombie process).
        let shutdown_notify = Arc::new(Notify::new());

        // 7. Start the HTTP and WebUI server (bound to the singleton port).

        let app =
            crate::web::server::create_app_with_config_path(&self.config, self.config_path.clone());
        let shutdown_for_server = shutdown_notify.clone();
        let server_handle = tokio::spawn(async move {
            match axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            {
                Ok(()) => {
                    error!("HTTP server exited unexpectedly");
                    shutdown_for_server.notify_one();
                }
                Err(e) => {
                    error!("HTTP server error: {}", e);
                    shutdown_for_server.notify_one();
                }
            }
        });
        info!(
            "HTTP server listening on http://{}:{}",
            self.config.bind_address, self.config.port
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
