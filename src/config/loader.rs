use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::model::GatewayConfig;
use crate::utils::env::substitute_env_vars;

pub struct ConfigLoader;

impl ConfigLoader {
    /// Prepare `~/.cc-gateway` (and `logs/`) then load `config.json` if present.
    /// Does **not** create `config.json` — only `cc-gateway init` (any exit) or
    /// an explicit save (e.g. WebUI) writes that file.
    pub fn load() -> Result<GatewayConfig> {
        Self::initialize_runtime()?;
        let path = Self::config_path()?;
        if path.is_file() {
            Self::load_from(&path)
        } else {
            Ok(GatewayConfig::runtime_defaults())
        }
    }

    /// Create config and log directories without writing `config.json`.
    pub fn initialize_runtime() -> Result<PathBuf> {
        let dir = Self::ensure_config_dir()?;
        fs::create_dir_all(dir.join("logs"))
            .with_context(|| format!("Failed to create logs dir {}", dir.display()))?;
        Ok(dir)
    }

    pub fn load_from(path: &Path) -> Result<GatewayConfig> {
        let raw = Self::parse_raw_from(path)?;
        let upgraded = upgrade_config_json(raw);
        serde_json::from_value(upgraded)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    fn parse_raw_from(path: &Path) -> Result<Value> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let substituted = substitute_env_vars(&content);
        serde_json::from_str(&substituted)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))
    }

    pub fn save(config: &GatewayConfig) -> Result<()> {
        let path = Self::config_path()?;
        Self::ensure_config_dir()?;
        let content = serde_json::to_string_pretty(config)?;
        fs::write(&path, content)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
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

/// One-shot upgrade of legacy on-disk JSON shapes into canonical `agent` profiles.
pub fn upgrade_config_json(mut value: Value) -> Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };

    if let Some(claude) = obj.remove("claude") {
        if !obj.contains_key("agent") {
            let default_args = claude
                .get("default_args")
                .and_then(|v| v.as_str())
                .unwrap_or("--dangerously-skip-permissions");
            obj.insert(
                "agent".to_string(),
                json!({
                    "default": "claude",
                    "claude": { "default_args": default_args },
                    "cursor": {},
                    "pi": {}
                }),
            );
        }
    }

    if let Some(agent) = obj.get("agent").cloned() {
        if agent.get("provider").is_some() && agent.get("default").is_none() {
            let provider = agent
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("claude");
            let mut profiles = json!({
                "default": provider,
                "claude": {},
                "cursor": {},
                "pi": {}
            });
            let profile_key = match provider {
                "cursor" => "cursor",
                "pi" => "pi",
                _ => "claude",
            };
            if let Some(profiles_obj) = profiles.as_object_mut() {
                if let Some(profile) = profiles_obj
                    .get_mut(profile_key)
                    .and_then(|v| v.as_object_mut())
                {
                    for key in ["default_args", "mode", "permission"] {
                        if let Some(v) = agent.get(key) {
                            profile.insert(key.to_string(), v.clone());
                        }
                    }
                }
            }
            obj.insert("agent".to_string(), profiles);
        }
    }

    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{AgentProvider, GatewayConfig};

    #[test]
    fn upgrade_top_level_claude_to_agent_profiles() {
        let raw = json!({
            "claude": {
                "cli_path": "custom-claude",
                "default_args": "--foo"
            }
        });
        let upgraded = upgrade_config_json(raw);
        let config: GatewayConfig = serde_json::from_value(upgraded).unwrap();
        assert_eq!(config.agent.default, AgentProvider::Claude);
        assert_eq!(config.agent.claude.default_args.as_deref(), Some("--foo"));
    }

    #[test]
    fn save_writes_config_json() {
        let dir = std::env::temp_dir().join(format!("cc-gateway-save-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &dir);

        let config_path = dir.join(".cc-gateway").join("config.json");
        assert!(!config_path.is_file());
        ConfigLoader::save(&GatewayConfig::runtime_defaults()).unwrap();
        assert!(config_path.is_file());

        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_does_not_create_config_json() {
        let dir = std::env::temp_dir().join(format!("cc-gateway-load-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let original_home = std::env::var("HOME").ok();
        std::env::set_var("HOME", &dir);

        let config_path = dir.join(".cc-gateway").join("config.json");
        assert!(!config_path.is_file());

        let config = ConfigLoader::load().unwrap();
        assert!(!config_path.is_file());
        assert!(!config.agent.claude.enabled);
        assert!(ConfigLoader::initialize_runtime().is_ok());

        match original_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upgrade_flat_agent_to_profiles() {
        let raw = json!({
            "agent": {
                "provider": "cursor",
                "cli_path": "cursor-agent",
                "default_args": "--force"
            }
        });
        let upgraded = upgrade_config_json(raw);
        let config: GatewayConfig = serde_json::from_value(upgraded).unwrap();
        assert_eq!(
            config.agent.effective_config().provider,
            AgentProvider::Cursor
        );
        assert_eq!(config.agent.effective_config().cli_path, "agent");
    }
}
