use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::model::GatewayConfig;
use crate::utils::env::substitute_env_vars;

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load() -> Result<GatewayConfig> {
        let path = Self::config_path()?;
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<GatewayConfig> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let substituted = substitute_env_vars(&content);
        let config: GatewayConfig = serde_json::from_str(&substituted)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        Ok(config)
    }

    pub fn config_path() -> Result<PathBuf> {
        let path = dirs::home_dir()
            .context("Could not determine home directory")?
            .join(".cc-gateway")
            .join("config.json");
        Ok(path)
    }

    pub fn ensure_config_dir() -> Result<PathBuf> {
        let dir = dirs::home_dir()
            .context("Could not determine home directory")?
            .join(".cc-gateway");
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

pub fn open_config_editor() -> Result<()> {
    let path = ConfigLoader::config_path()?;
    if !path.exists() {
        let default = GatewayConfig::default();
        let content = serde_json::to_string_pretty(&default)?;
        ConfigLoader::ensure_config_dir()?;
        fs::write(&path, content)?;
        println!("Created default config at: {}", path.display());
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    std::process::Command::new(editor)
        .arg(&path)
        .status()
        .context("Failed to open editor")?;
    Ok(())
}
