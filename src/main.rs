//! `cc-gateway` binary entry point.
//!
//! This file is only responsible for two things:
//!
//! 1. Parsing command line subcommands using `clap`;
//! 2. Routing each subcommand to the corresponding implementation in the [`cc_gateway`] library (see [`lib.rs`](../lib.rs)).
//!
//! **All business logic is located in the `cc_gateway` library**, keeping `main.rs` as a thin shell. This allows
//! integration tests to import the library directly, and simplifies future Rust API reuse (e.g., embedding into other tools).

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

/// Top-level command line arguments.
///
/// If no subcommand is provided, it shows help (`--help`) and exits with 0.
#[derive(Parser, Debug)]
#[command(name = "cc-gateway")]
#[command(
    about = "Gateway for controlling local agent CLIs via Feishu/Lark, Telegram, QQ, and WebUI"
)]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// All subcommands.
///
/// `Start` / `Stop` / `Restart` / `Log` / `Status` / `Enable` / `Disable` are user-facing
/// daemon operations; `Init` is the initial configuration wizard; `Webui` / `WebuiToken` are WebUI related;
/// `Update` / `Uninstall` are maintenance commands; `Daemon` / `McpServer` are **internal** subcommands
/// (annotated with `hide = true`, automatically invoked when `start()` spawns the daemon process).
#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the gateway daemon process (forks a detached child process, current process exits).
    Start {
        /// Configuration file path (defaults to `~/.cc-gateway/config.json` if omitted).
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Gracefully stop the daemon process (sends SIGTERM, waits for child process exit).
    Stop,
    /// Restart the daemon process (stops then starts).
    Restart {
        /// Configuration file path.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// View the daemon process logs.
    Log {
        /// Follow log output (equivalent to `tail -f`).
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to display initially.
        #[arg(short = 'n', long, default_value = "100")]
        lines: usize,
    },
    /// Display the daemon process status (PID, port, active status).
    Status,
    /// Enable autostart on system boot (launchd / systemd / Task Scheduler).
    Enable,
    /// Disable autostart on system boot.
    Disable,
    /// Interactively initialize configuration (`~/.cc-gateway/config.json`).
    Init,
    /// Open the WebUI in the default browser; automatically starts the daemon if it's not running.
    Webui {
        /// Configuration file path.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Display the WebUI access token; `--refresh` generates a new token (invalidates the old one).
    WebuiToken {
        /// Generate a new token (invalidates the old one).
        #[arg(short, long)]
        refresh: bool,
    },
    /// Check for updates and optionally install the latest version (pulls from GitHub Releases).
    Update {
        /// Check only; do not download or install.
        #[arg(long)]
        check: bool,
        /// Force update (even if already on the latest version).
        #[arg(short, long)]
        force: bool,
        /// Do not prompt for confirmation during installation.
        #[arg(short = 'y', long)]
        yes: bool,
        /// Configuration file path used to restart the daemon after updating.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Uninstall cc-gateway (binary, autostart, PATH, and data directory).
    Uninstall {
        /// Do not prompt for confirmation.
        #[arg(short = 'y', long)]
        yes: bool,
        /// Keep the data directory (`~/.cc-gateway`) and do not delete it.
        #[arg(long)]
        keep_data: bool,
    },
    /// Internal: Run the daemon engine (do not invoke directly; spawned by `start()`).
    #[command(hide = true, name = "_daemon")]
    Daemon {
        /// Configuration file path.
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Run the MCP server for Claude Code (do not invoke directly).
    #[command(hide = true, name = "_mcp-server")]
    McpServer,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let Some(command) = args.command else {
        // Show help if no subcommand is provided (equivalent to `--help`), exit 0.
        Args::command().print_help()?;
        return Ok(());
    };

    // Initialize i18n (sets current language based on user locale).
    cc_gateway::i18n::init();

    match command {
        Commands::Start { config } => {
            cc_gateway::daemon::start(config).await?;
        }
        Commands::Stop => {
            cc_gateway::daemon::stop().await?;
        }
        Commands::Restart { config } => {
            cc_gateway::daemon::restart(config).await?;
        }
        Commands::Daemon { config } => {
            // Spawned by `start()` inside the daemon fork/exec wrapper to run the main daemon loop.
            cc_gateway::daemon::run(config).await?;
        }
        Commands::Log { follow, lines } => {
            cc_gateway::daemon::log(follow, lines).await?;
        }
        Commands::Status => {
            cc_gateway::daemon::status().await?;
        }
        Commands::Enable => {
            cc_gateway::daemon::enable().await?;
        }
        Commands::Disable => {
            cc_gateway::daemon::disable().await?;
        }
        Commands::Init => {
            // Interactive configuration wizard (synchronous call).
            cc_gateway::config::wizard::run_init_config()?;
        }
        Commands::Webui { config } => {
            cc_gateway::daemon::webui(config).await?;
        }
        Commands::WebuiToken { refresh } => {
            cc_gateway::daemon::webui_token(refresh).await?;
        }
        Commands::Update {
            check,
            force,
            yes,
            config,
        } => {
            cc_gateway::update::run(check, force, yes, config).await?;
        }
        Commands::Uninstall { yes, keep_data } => {
            cc_gateway::uninstall::run(yes, keep_data)?;
        }
        Commands::McpServer => {
            // Internal command: Runs as a stdio MCP server for Claude Code mounting.
            cc_gateway::runtime::mcp_server::run_mcp_server().await?;
        }
    }

    Ok(())
}
