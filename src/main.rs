use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod cli;
mod claude;
mod command;
mod config;
mod daemon;
mod i18n;
mod platform;
mod prompt;
mod skill;
mod utils;

use cli::interactive::run_interactive;

#[derive(Parser)]
#[command(name = "cc-gateway")]
#[command(about = "Gateway for controlling Claude Code via Feishu/Lark and CLI")]
#[command(version)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the gateway daemon
    Start {
        /// Config file path
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Stop the gateway daemon
    Stop,
    /// Restart the gateway daemon
    Restart {
        /// Config file path
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// View daemon logs
    Log {
        /// Follow log output
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show
        #[arg(short = 'n', long, default_value = "100")]
        lines: usize,
    },
    /// Show daemon status
    Status,
    /// Enable auto-start on boot
    Enable,
    /// Disable auto-start on boot
    Disable,
    /// Initialize configuration interactively
    Init,
    /// Edit configuration
    Config {
        /// Print default config to stdout
        #[arg(long)]
        init: bool,
    },
    /// Internal: run the daemon engine (do not use directly)
    #[command(hide = true, name = "_daemon")]
    Daemon {
        /// Config file path
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    i18n::init();
    let args = Args::parse();

    match args.command {
        None => {
            // No subcommand: enter interactive mode (same as Feishu bot)
            run_interactive().await?;
        }
        Some(Commands::Start { config }) => {
            daemon::start(config).await?;
        }
        Some(Commands::Stop) => {
            daemon::stop().await?;
        }
        Some(Commands::Restart { config }) => {
            daemon::restart(config).await?;
        }
        Some(Commands::Daemon { config }) => {
            daemon::run(config).await?;
        }
        Some(Commands::Log { follow, lines }) => {
            daemon::log(follow, lines).await?;
        }
        Some(Commands::Status) => {
            daemon::status().await?;
        }
        Some(Commands::Enable) => {
            daemon::enable().await?;
        }
        Some(Commands::Disable) => {
            daemon::disable().await?;
        }
        Some(Commands::Init) => {
            config::wizard::run_init_config()?;
        }
        Some(Commands::Config { init }) => {
            if init {
                let default = config::model::GatewayConfig::default();
                println!("{}", serde_json::to_string_pretty(&default)?);
            } else {
                config::wizard::run_interactive_config()?;
            }
        }
    }

    Ok(())
}
