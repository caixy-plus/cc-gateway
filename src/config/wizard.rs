use anyhow::{Context, Result};
use std::io::{self, Write};

use crate::config::loader::ConfigLoader;
use crate::config::model::{
    AgentProfiles, AgentProviderConfig, FeishuConfig, GatewayConfig, LogConfig,
};
use crate::{t, t_fmt};

pub fn run_interactive_config() -> Result<()> {
    let mut config = match ConfigLoader::load() {
        Ok(c) => {
            println!(
                "Loaded existing config from: {}",
                ConfigLoader::config_path()?.display()
            );
            c
        }
        Err(_) => {
            println!("{}", t!("wizard.no_config_defaults"));
            GatewayConfig::default()
        }
    };

    loop {
        println!("\n{}\n", t!("wizard.title"));
        println!("  1. {}", t!("wizard.log_section"));
        println!("  2. {}", t!("wizard.agent_section"));
        println!("  3. {}", t!("wizard.feishu_section"));
        println!("  4. {}", t!("wizard.default_dir_section"));
        println!("  5. {}", t!("wizard.save_exit"));
        println!("  6. {}", t!("wizard.exit_no_save"));
        print!("\n{} ", t!("wizard.select_section"));
        io::stdout().flush()?;

        let mut choice = String::new();
        io::stdin().read_line(&mut choice)?;

        match choice.trim() {
            "1" => configure_log(&mut config.log)?,
            "2" => configure_agent(&mut config.agent)?,
            "3" => configure_feishu(&mut config.feishu)?,
            "4" => configure_default_dir(&mut config.default_dir)?,
            "5" => {
                save_config(&config)?;
                println!("{}", t!("wizard.config_saved"));
                break;
            }
            "6" => {
                println!("{}", t!("wizard.exiting_no_save"));
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
    println!("\n{}", t!("wizard.log_config"));
    log.level = prompt("level", &log.level)?;
    log.file = prompt("file", &log.file)?;
    Ok(())
}

fn configure_agent_profile(label: &str, profile: &mut AgentProviderConfig) -> Result<()> {
    println!("\n{}", t_fmt!("wizard.agent_profile", NAME = label));
    let cli_path = profile
        .cli_path
        .clone()
        .unwrap_or_else(|| "claude".to_string());
    let default_args = profile
        .default_args
        .clone()
        .unwrap_or_else(|| "--dangerously-skip-permissions".to_string());
    profile.cli_path = Some(prompt("cli_path", &cli_path)?);
    profile.default_args = Some(prompt("default_args", &default_args)?);
    Ok(())
}

fn configure_agent(agent: &mut AgentProfiles) -> Result<()> {
    println!("\n{}", t!("wizard.agent_config"));
    configure_agent_profile("claude", &mut agent.claude)?;
    configure_agent_profile("cursor", &mut agent.cursor)?;
    Ok(())
}

fn configure_feishu(feishu: &mut FeishuConfig) -> Result<()> {
    println!("\n{}", t!("wizard.feishu_config"));
    feishu.enabled = prompt_bool("enabled", feishu.enabled)?;
    feishu.app_id = prompt("app_id", &feishu.app_id)?;
    feishu.app_secret = prompt_sensitive("app_secret", &feishu.app_secret)?;
    feishu.allow_from = prompt("allow_from", &feishu.allow_from)?;
    feishu.encrypt_key = prompt("encrypt_key", &feishu.encrypt_key)?;
    Ok(())
}

fn configure_default_dir(default_dir: &mut String) -> Result<()> {
    println!("\n{}", t!("wizard.default_dir_config"));
    *default_dir = prompt("default_dir", default_dir)?;
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
    ConfigLoader::save(config)
        .with_context(|| format!("Failed to write config to {}", path.display()))
}

pub fn run_init_config() -> Result<()> {
    let config_path = ConfigLoader::config_path()?;

    if config_path.is_file() {
        println!(
            "{}",
            t_fmt!("wizard.init_skipped_existing", PATH = config_path.display())
        );
        return Ok(());
    }

    ConfigLoader::ensure_config_dir()?;

    println!("{}\n", t!("wizard.init_title"));
    println!("{}", t!("wizard.welcome"));
    println!(
        "{}",
        t_fmt!("wizard.location", PATH = config_path.display())
    );
    println!("{}\n", t!("wizard.rerun"));
    println!("{}\n", t!("wizard.no_config_defaults"));

    let mut config = GatewayConfig::default();

    println!("{}", t!("wizard.feishu_section_title"));
    println!("{}", t!("wizard.press_enter_keep"));
    println!("{}\n", t!("wizard.enter_skip"));

    let app_id = prompt_with_skip("app_id", &config.feishu.app_id)?;
    let skip_feishu = app_id == "skip";

    if !skip_feishu {
        config.feishu.app_id = app_id;
        config.feishu.app_secret = prompt_sensitive("app_secret", &config.feishu.app_secret)?;
        config.feishu.allow_from = prompt("allow_from", &config.feishu.allow_from)?;
        config.feishu.encrypt_key = prompt("encrypt_key", &config.feishu.encrypt_key)?;
        config.feishu.mode = prompt("mode", &config.feishu.mode)?;
        config.feishu.webhook_bind = prompt("webhook_bind", &config.feishu.webhook_bind)?;
    }

    println!("\n{}", t!("wizard.other_settings"));
    let default_dir = prompt("default_dir", &config.default_dir)?;
    if !default_dir.is_empty() {
        config.default_dir = default_dir;
    }

    save_config(&config)?;

    println!("\n{}", t!("wizard.setup_complete"));
    println!(
        "{}",
        t_fmt!("wizard.config_saved_to", PATH = config_path.display())
    );
    println!("\n{}", t!("wizard.modify_later"));
    println!("{}", t!("wizard.run_config"));
    println!("{}", t_fmt!("wizard.or_edit", PATH = config_path.display()));

    if skip_feishu || config.feishu.app_id.is_empty() || config.feishu.app_id.starts_with("${") {
        println!("\n{}", t!("wizard.feishu_not_configured"));
        println!("{}", t!("wizard.without_credentials"));
        println!("{}", t!("wizard.configure_later"));
    }

    Ok(())
}

fn prompt_with_skip(label: &str, current: &str) -> Result<String> {
    print!("{} [{}] (type 'skip' to skip): ", label, current);
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
