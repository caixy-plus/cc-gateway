use anyhow::Result;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::claude::controller::ClaudeController;
use crate::config::loader::ConfigLoader;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use crate::t;

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

pub fn format_banner() -> String {
    t!("cli.banner").to_string()
}

// ---------------------------------------------------------------------------
// Real terminal entry point
// ---------------------------------------------------------------------------

pub async fn run_interactive() -> Result<()> {
    let config = ConfigLoader::load()?;
    let default_dir = config.default_dir.clone();

    crate::db::init_schema()?;
    GLOBAL_CHANNEL_SESSIONS.load_from_db();

    // Get or create the implicit TUI channel
    let channel = GLOBAL_CHANNEL_SESSIONS
        .get_or_create_platform_channel("tui", "tui", &default_dir)
        .await;
    let channel_id = channel.id.clone();

    // Create a controller without auto-starting a Claude session.
    // The user must type /claude to start one.
    let controller = Arc::new(Mutex::new(ClaudeController::new(
        config.claude.clone(),
        config.show_thinking,
    )));

    // Set initial work_dir to current directory
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| default_dir.clone());
    {
        let ctrl = controller.lock().await;
        ctrl.init_work_dir(cwd).await;
    }

    let router = crate::command::router::CommandRouter::new(controller.clone(), &default_dir);

    crate::cli::tui::run_tui(controller, router, channel_id).await
}
