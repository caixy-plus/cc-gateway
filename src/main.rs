use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cc-gateway")]
#[command(
    about = "Gateway for controlling local agent CLIs via Feishu/Lark, Telegram, QQ, and WebUI"
)]
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
    /// Open WebUI in the default browser (starts daemon if not running)
    Webui {
        /// Config file path
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Show or refresh the WebUI access token
    WebuiToken {
        /// Generate a new token (invalidates the old one)
        #[arg(short, long)]
        refresh: bool,
    },
    /// Check for updates and optionally install the latest release
    Update {
        /// Only check, do not download or install
        #[arg(long)]
        check: bool,
        /// Force update even if already on the latest version
        #[arg(short, long)]
        force: bool,
        /// Install without prompting for confirmation
        #[arg(short = 'y', long)]
        yes: bool,
        /// Config file path used when restarting after update
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Uninstall cc-gateway (binary, auto-start, PATH entry, and data)
    Uninstall {
        /// Skip the confirmation prompt
        #[arg(short = 'y', long)]
        yes: bool,
        /// Keep the data directory (~/.cc-gateway) instead of deleting it
        #[arg(long)]
        keep_data: bool,
    },
    /// Internal: run the daemon engine (do not use directly)
    #[command(hide = true, name = "_daemon")]
    Daemon {
        /// Config file path
        #[arg(short, long)]
        config: Option<PathBuf>,
    },
    /// Internal: run MCP server for Claude Code (do not use directly)
    #[command(hide = true, name = "_mcp-server")]
    McpServer,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let Some(command) = args.command else {
        Args::command().print_help()?;
        return Ok(());
    };

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
            cc_gateway::runtime::mcp_server::run_mcp_server().await?;
        }
    }

    Ok(())
}
