use anyhow::Result;
use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Result as RustylineResult};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::claude::controller::ClaudeController;
use crate::command::router::CommandRouter;
use crate::config::loader::ConfigLoader;
use crate::config::model::GatewayConfig;

pub async fn run_interactive() -> Result<()> {
    println!("cc-gateway interactive mode");
    println!("Type '/help' for available commands, '/quit' to exit.\n");

    let config = ConfigLoader::load()?;
    let controller = Arc::new(Mutex::new(ClaudeController::new(config.claude.clone())));
    let router = CommandRouter::new(controller.clone());

    let mut rl = DefaultEditor::new()?;
    let prompt = "cc-gateway> ";

    loop {
        let readline = rl.readline(prompt);
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line);

                if let Some(response) = router.handle(line).await {
                    println!("{}", response);
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}
