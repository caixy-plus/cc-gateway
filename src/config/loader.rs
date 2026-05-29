use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::model::GatewayConfig;
use crate::utils::env::substitute_env_vars;

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn load() -> Result<GatewayConfig> {
        // Guarantee a config file always exists, even if the user bypassed
        // `cc-gateway init` and started the daemon / TUI / WebUI directly. Once
        // the file exists, `init` is skipped and all further changes happen in
        // the WebUI.
        let path = Self::ensure_config_file()?;
        Self::load_from(&path)
    }

    /// Create a default config file at the standard path if one does not exist.
    /// Idempotent: never overwrites an existing file. Returns the config path.
    pub fn ensure_config_file() -> Result<PathBuf> {
        let path = Self::config_path()?;
        Self::ensure_config_file_at(&path)?;
        Ok(path)
    }

    /// Path-parameterized core of [`ensure_config_file`], split out for testing.
    fn ensure_config_file_at(path: &Path) -> Result<()> {
        if path.is_file() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(&GatewayConfig::default())?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write default config to {}", path.display()))?;
        Ok(())
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
            let cli_path = claude
                .get("cli_path")
                .and_then(|v| v.as_str())
                .unwrap_or("claude");
            let default_args = claude
                .get("default_args")
                .and_then(|v| v.as_str())
                .unwrap_or("--dangerously-skip-permissions");
            obj.insert(
                "agent".to_string(),
                json!({
                    "default": "claude",
                    "claude": { "cli_path": cli_path, "default_args": default_args },
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
                    for key in ["cli_path", "default_args", "mode", "permission"] {
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
        assert_eq!(
            config.agent.claude.cli_path.as_deref(),
            Some("custom-claude")
        );
        assert_eq!(config.agent.claude.default_args.as_deref(), Some("--foo"));
    }

    #[test]
    fn ensure_config_file_creates_default_then_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("cc-gateway-ensure-{}", std::process::id()));
        let path = dir.join("nested").join("config.json");
        let _ = fs::remove_dir_all(&dir);

        // First call creates a parseable default config (and parent dirs).
        assert!(!path.is_file());
        ConfigLoader::ensure_config_file_at(&path).unwrap();
        assert!(path.is_file());
        let config = ConfigLoader::load_from(&path).unwrap();
        assert_eq!(config.port, GatewayConfig::default().port);

        // Second call must NOT overwrite a user-modified file.
        fs::write(&path, r#"{"port": 12345}"#).unwrap();
        ConfigLoader::ensure_config_file_at(&path).unwrap();
        assert_eq!(ConfigLoader::load_from(&path).unwrap().port, 12345);

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
        assert_eq!(config.agent.effective_config().cli_path, "cursor-agent");
    }
}
