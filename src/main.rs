use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tracing::{info, warn};

mod ai;
mod cli;
mod claude;
mod command;
mod config;
mod daemon;
mod platform;
mod utils;

use cli::interactive::run_interactive;
use config::loader::ConfigLoader;

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
        #[arg(short, long, default_value = "100")]
        lines: usize,
    },
    /// Edit configuration
    Config {
        /// Print default config to stdout
        #[arg(long)]
        init: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
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
        Some(Commands::Log { follow, lines }) => {
            daemon::log(follow, lines).await?;
        }
        Some(Commands::Config { init }) => {
            if init {
                let default = config::model::GatewayConfig::default();
                println!("{}", serde_json::to_string_pretty(&default)?);
            } else {
                config::loader::open_config_editor()?;
            }
        }
    }

    Ok(())
}
