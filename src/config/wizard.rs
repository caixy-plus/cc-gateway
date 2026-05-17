use anyhow::{Context, Result};
use std::io::{self, Write};

use crate::config::loader::ConfigLoader;
use crate::config::model::{ClaudeConfig, FeishuConfig, GatewayConfig, LogConfig};

pub fn run_interactive_config() -> Result<()> {
    let mut config = match ConfigLoader::load() {
        Ok(c) => {
            println!("Loaded existing config from: {}", ConfigLoader::config_path()?.display());
            c
        }
        Err(_) => {
            println!("No existing config found. Starting with defaults.");
            GatewayConfig::default()
        }
    };

    loop {
        println!("\n=== cc-gateway Configuration ===\n");
        println!("  1. log       - Logging settings");
        println!("  2. claude    - Claude Code settings");
        println!("  3. feishu    - Feishu/Lark bot settings");
        println!("  4. Save and exit");
        println!("  5. Exit without saving");
        print!("\nSelect section [1-5]: ");
        io::stdout().flush()?;

        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => configure_log(&mut config.log)?,
            "2" => configure_claude(&mut config.claude)?,
            "3" => configure_feishu(&mut config.feishu)?,
            "4" => {
                save_config(&config)?;
                println!("Config saved.");
                break;
            }
            "5" => {
                println!("Exiting without saving.");
                break;
            }
            _ => println!("Invalid choice, try again."),
        }
    }

    Ok(())
}

fn prompt(label: &str, current: &str) -> Result<String> {
    print!("{} [{}]: ", label, current);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(current.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_bool(label: &str, current: bool) -> Result<bool> {
    let default = if current { "true" } else { "false" };
    print!("{} [{}]: ", label, default);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        Ok(current)
    } else {
        Ok(trimmed == "true" || trimmed == "yes" || trimmed == "1")
    }
}

fn configure_log(log: &mut LogConfig) -> Result<()> {
    println!("\n--- Log Configuration ---");
    log.level = prompt("level", &log.level)?;
    log.file = prompt("file", &log.file)?;
    Ok(())
}

fn configure_claude(claude: &mut ClaudeConfig) -> Result<()> {
    println!("\n--- Claude Configuration ---");
    claude.cli_path = prompt("cli_path", &claude.cli_path)?;
    claude.default_args = prompt("default_args", &claude.default_args)?;
    Ok(())
}

fn configure_feishu(feishu: &mut FeishuConfig) -> Result<()> {
    println!("\n--- Feishu Configuration ---");
    feishu.enabled = prompt_bool("enabled", feishu.enabled)?;
    feishu.app_id = prompt("app_id", &feishu.app_id)?;
    feishu.app_secret = prompt_sensitive("app_secret", &feishu.app_secret)?;
    feishu.allow_from = prompt("allow_from", &feishu.allow_from)?;
    feishu.encrypt_key = prompt("encrypt_key", &feishu.encrypt_key)?;
    feishu.default_dir = prompt("default_dir", &feishu.default_dir)?;
    Ok(())
}

fn prompt_sensitive(label: &str, current: &str) -> Result<String> {
    if current.is_empty() || current.starts_with("${") {
        print!("{} [{}]: ", label, current);
    } else {
        let masked: String = current.chars().map(|_| '*').collect();
        print!("{} [{}]: ", label, masked);
    }
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(current.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn save_config(config: &GatewayConfig) -> Result<()> {
    let path = ConfigLoader::config_path()?;
    ConfigLoader::ensure_config_dir()?;
    let content = serde_json::to_string_pretty(config)?;
    std::fs::write(&path, content).with_context(|| format!("Failed to write config to {}", path.display()))?;
    Ok(())
}
