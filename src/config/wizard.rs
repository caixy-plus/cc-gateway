use anyhow::{Context, Result};
use std::io::{self, Write};

use crate::config::loader::ConfigLoader;
use crate::config::model::{AgentConfig, AgentProvider, GatewayConfig};
use crate::{t, t_fmt};

/// First-run guided setup. Skipped when `config.json` already exists.
///
/// Always writes `config.json` before returning — including wizard errors or
/// early interruption — so daemon / TUI / WebUI can load a real file afterward.
pub fn run_init_config() -> Result<()> {
    let config_path = ConfigLoader::config_path()?;

    if config_path.is_file() {
        println!(
            "{}",
            t_fmt!("wizard.init_skipped_existing", PATH = config_path.display())
        );
        return Ok(());
    }

    ConfigLoader::initialize_runtime()?;

    println!("{}\n", t!("wizard.init_title"));
    println!("{}", t!("wizard.welcome"));
    println!(
        "{}",
        t_fmt!("wizard.location", PATH = config_path.display())
    );
    println!("{}\n", t!("wizard.rerun"));

    let mut config = GatewayConfig::runtime_defaults();
    // Baseline on disk immediately so even SIGINT / I/O errors mid-wizard still
    // leave a usable config file (subsequent save refreshes wizard choices).
    save_config(&config)?;

    let mut warnings: Vec<String> = Vec::new();

    let wizard_result: Result<()> = (|| {
        configure_agent_step(&mut config, &mut warnings)?;
        configure_bot_step(&mut config, &mut warnings)?;

        config.webui_token = Some(crate::web::middleware::generate_webui_token());

        println!("\n{}", t!("wizard.review_title"));
        if warnings.is_empty() {
            println!("{}", t!("wizard.review_ok"));
        } else {
            println!("{}", t!("wizard.review_has_issues"));
            for w in &warnings {
                println!("{}", w);
            }
        }
        Ok(())
    })();

    save_config(&config).context("Failed to persist configuration after init")?;

    wizard_result?;

    println!("\n{}", t!("wizard.setup_complete"));
    println!(
        "{}",
        t_fmt!("wizard.config_saved_to", PATH = config_path.display())
    );
    println!("\n{}", t!("wizard.webui_from_now"));
    if let Some(ref token) = config.webui_token {
        println!(
            "  {}",
            t_fmt!("wizard.webui_token_generated", TOKEN = token.as_str())
        );
    }

    Ok(())
}

/// Step 1: pick a single agent provider (or skip). Both can be configured later
/// in the WebUI.
fn configure_agent_step(config: &mut GatewayConfig, warnings: &mut Vec<String>) -> Result<()> {
    println!("\n{}", t!("wizard.agent_section_title"));
    println!("{}", t!("wizard.agent_section_hint"));
    println!("  1. claude    {}", status_label(cli_installed("claude")));
    println!("  2. cursor    {}", status_label(cli_installed("agent")));
    println!("  3. pi        {}", status_label(cli_installed("pi")));
    println!("  4. codew {}", status_label(cli_installed("codewhale")));
    println!("  {}", t!("wizard.opt_skip"));

    let provider = match read_choice()?.as_str() {
        "1" | "claude" => AgentProvider::Claude,
        "2" | "cursor" => AgentProvider::Cursor,
        "3" | "pi" => AgentProvider::Pi,
        "4" | "codew" => AgentProvider::CodeWhale,
        _ => {
            println!("{}", t!("wizard.skipped_agent"));
            return Ok(());
        }
    };

    config.agent.default = provider.clone();
    let defaults = AgentConfig::default_for_provider(provider.clone());
    let default_args = prompt_field("default_args", &defaults.default_args)?;

    let target = match provider {
        AgentProvider::Claude => &mut config.agent.claude,
        AgentProvider::Cursor => &mut config.agent.cursor,
        AgentProvider::Pi => &mut config.agent.pi,
        AgentProvider::CodeWhale => &mut config.agent.codewhale,
    };
    target.enabled = true;
    target.default_args = Some(default_args);

    if !cli_installed(&defaults.cli_path) {
        println!(
            "{}",
            t_fmt!("wizard.agent_unavailable_warn", NAME = defaults.cli_path)
        );
        warnings.push(t_fmt!(
            "wizard.warn_agent_missing",
            NAME = provider.to_string()
        ));
    }
    println!(
        "{}",
        t_fmt!("wizard.agent_configured", NAME = provider.to_string())
    );
    Ok(())
}

/// Step 2: pick a single bot platform (or skip). Both can be configured later
/// in the WebUI.
fn configure_bot_step(config: &mut GatewayConfig, warnings: &mut Vec<String>) -> Result<()> {
    println!("\n{}", t!("wizard.bot_section_title"));
    println!("{}", t!("wizard.bot_section_hint"));
    println!("  1. feishu");
    println!("  2. telegram");
    println!("  {}", t!("wizard.opt_skip"));

    match read_choice()?.as_str() {
        "1" | "feishu" => {
            config.feishu.enabled = true;
            config.feishu.app_id = prompt_field("app_id", "")?;
            config.feishu.app_secret = prompt_field("app_secret", "")?;
            if config.feishu.app_id.is_empty() || config.feishu.app_secret.is_empty() {
                warnings.push(t!("wizard.warn_feishu_incomplete").to_string());
            }
            println!("{}", t_fmt!("wizard.bot_configured", NAME = "feishu"));
        }
        "2" | "telegram" => {
            config.telegram.enabled = true;
            config.telegram.bot_token = prompt_field("bot_token", "")?;
            if config.telegram.bot_token.is_empty() {
                warnings.push(t!("wizard.warn_telegram_incomplete").to_string());
            }
            println!("{}", t_fmt!("wizard.bot_configured", NAME = "telegram"));
        }
        _ => {
            println!("{}", t!("wizard.skipped_bot"));
        }
    }
    Ok(())
}

/// Whether a CLI is resolvable to an existing executable on this machine.
fn cli_installed(name: &str) -> bool {
    let resolved = crate::runtime::session::resolve_cli_path(name);
    let p = std::path::Path::new(&resolved);
    p.is_absolute() && p.exists()
}

fn status_label(installed: bool) -> &'static str {
    if installed {
        t!("wizard.label_installed")
    } else {
        t!("wizard.label_not_found")
    }
}

fn read_line_trimmed() -> Result<String> {
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn read_choice() -> Result<String> {
    print!("  {} ", t!("wizard.choose_prompt"));
    io::stdout().flush()?;
    Ok(read_line_trimmed()?.to_lowercase())
}

/// Prompt for a field with an auto-filled default.
/// - Enter keeps the default (which may be empty).
/// - Typing `-` clears the value to empty.
/// - Any other input overrides the default.
fn prompt_field(label: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        print!("  {} []: ", label);
    } else {
        print!("  {} [{}] {}: ", label, default, t!("wizard.keep_or_clear"));
    }
    io::stdout().flush()?;
    let input = read_line_trimmed()?;
    Ok(match input.as_str() {
        "" => default.to_string(),
        "-" => String::new(),
        other => other.to_string(),
    })
}

fn save_config(config: &GatewayConfig) -> Result<()> {
    let path = ConfigLoader::config_path()?;
    ConfigLoader::save(config)
        .with_context(|| format!("Failed to write config to {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_defaults_disables_all_integrations() {
        let config = GatewayConfig::runtime_defaults();

        assert!(!config.agent.claude.enabled);
        assert!(!config.agent.cursor.enabled);
        assert!(!config.agent.pi.enabled);
        assert!(!config.agent.codewhale.enabled);
        assert!(!config.feishu.enabled);
        assert!(!config.telegram.enabled);
    }

}
