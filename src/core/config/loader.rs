use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::config::model::GatewayConfig;
use crate::utils::env::substitute_env_vars;

pub struct ConfigLoader;

impl ConfigLoader {
    /// Prepare `~/.cc-gateway` (and `logs/`) then load `config.json` if present.
    /// Does **not** create `config.json` when the file is missing — only `cc-gateway init`
    /// (any exit) or an explicit save (e.g. WebUI) writes a new file. When the file exists,
    /// legacy shapes are upgraded in memory and **written back** if the on-disk structure
    /// changed (platform nesting, flat `agent`, missing registry provider profiles, etc.).
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
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let disk_value: Value = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        let upgraded_disk = upgrade_config_json(disk_value.clone());
        let structure_migrated = upgraded_disk != disk_value;

        let substituted = substitute_env_vars(&content);
        let raw = serde_json::from_str(&substituted)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        let upgraded = upgrade_config_json(raw);
        crate::config::agent_registry::validate_agent_profile_keys(&upgraded)?;
        let mut config: GatewayConfig = serde_json::from_value(upgraded)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        let agent_before = config.agent.clone();
        config.agent = crate::config::agent_registry::normalize_profiles(config.agent);
        let profiles_normalized = config.agent != agent_before;

        if structure_migrated || profiles_normalized {
            info!(
                "Migrating config structure; writing updated file to {}",
                path.display()
            );
            Self::save_to(path, &config)?;
        }

        Ok(config)
    }

    pub fn save(config: &GatewayConfig) -> Result<()> {
        let path = Self::config_path()?;
        Self::ensure_config_dir()?;
        Self::save_to(&path, config)
    }

    pub fn save_to(path: &Path, config: &GatewayConfig) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(config)?;
        fs::write(path, content)
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
                    "providers": {
                        "claude": { "default_args": default_args },
                        "cursor": {},
                        "pi": {}
                    }
                }),
            );
        }
    }

    if let Some(agent) = obj.get_mut("agent").and_then(|a| a.as_object_mut()) {
        agent.remove("codewhale");
        if let Some(providers) = agent.get_mut("providers").and_then(|p| p.as_object_mut()) {
            providers.remove("codewhale");
        }
        if matches!(
            agent.get("default").and_then(|v| v.as_str()),
            Some("codew") | Some("codewhale")
        ) {
            agent.insert("default".to_string(), json!("claude"));
        }
    }

    migrate_platform_sections_to_platforms(obj);

    if let Some(agent) = obj.get("agent").cloned() {
        if agent.get("provider").is_some() && agent.get("default").is_none() {
            let provider = agent
                .get("provider")
                .and_then(|v| v.as_str())
                .unwrap_or("claude");
            let profile_key = match provider {
                "cursor" => "cursor",
                "pi" => "pi",
                _ => "claude",
            };
            let mut profile = json!({});
            if let Some(profile_obj) = profile.as_object_mut() {
                for key in ["default_args", "mode", "permission"] {
                    if let Some(v) = agent.get(key) {
                        profile_obj.insert(key.to_string(), v.clone());
                    }
                }
            }
            obj.insert(
                "agent".to_string(),
                json!({
                    "default": provider,
                    "providers": {
                        "claude": if profile_key == "claude" { profile.clone() } else { json!({}) },
                        "cursor": if profile_key == "cursor" { profile.clone() } else { json!({}) },
                        "pi": if profile_key == "pi" { profile } else { json!({}) }
                    }
                }),
            );
        }
    }

    if let Some(agent) = obj.get_mut("agent").and_then(|a| a.as_object_mut()) {
        migrate_agent_profiles_to_nested(agent);
    }

    value
}

/// Move legacy flat `agent.<provider>` keys under `agent.providers`.
fn migrate_agent_profiles_to_nested(agent: &mut serde_json::Map<String, Value>) {
    const RESERVED: [&str; 2] = ["default", "providers"];
    let extra_keys: Vec<String> = agent
        .keys()
        .filter(|k| !RESERVED.contains(&k.as_str()))
        .cloned()
        .collect();

    if extra_keys.is_empty() {
        agent
            .entry("providers".to_string())
            .or_insert_with(|| json!({}));
        return;
    }

    let moved: Vec<(String, Value)> = extra_keys
        .into_iter()
        .filter_map(|key| agent.remove(&key).map(|v| (key, v)))
        .collect();

    let (provider_entries, stray_entries): (Vec<_>, Vec<_>) =
        moved.into_iter().partition(|(_, v)| v.is_object());

    let providers = agent
        .entry("providers".to_string())
        .or_insert_with(|| json!({}));
    if let Some(providers_obj) = providers.as_object_mut() {
        for (key, v) in provider_entries {
            providers_obj.entry(key).or_insert(v);
        }
    } else {
        for (key, v) in provider_entries {
            agent.insert(key, v);
        }
    }

    for (key, v) in stray_entries {
        agent.insert(key, v);
    }
}

/// Move legacy top-level `feishu` / `telegram` / `qq` into `"platforms": { … }`.
fn migrate_platform_sections_to_platforms(obj: &mut serde_json::Map<String, Value>) {
    const KEYS: [&str; 3] = ["feishu", "telegram", "qq"];
    let mut legacy: serde_json::Map<String, Value> = serde_json::Map::new();
    for key in KEYS {
        if let Some(v) = obj.remove(key) {
            legacy.insert(key.to_string(), v);
        }
    }
    if legacy.is_empty() {
        return;
    }
    let platforms = obj
        .entry("platforms".to_string())
        .or_insert_with(|| json!({}));
    if let Some(existing) = platforms.as_object_mut() {
        for (k, v) in legacy {
            existing.entry(k).or_insert(v);
        }
    } else {
        obj.insert("platforms".to_string(), Value::Object(legacy));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{AgentProvider, GatewayConfig};
    use crate::tests::helpers::TestEnv;

    #[test]
    fn upgrade_strips_removed_codewhale_profile() {
        let raw = json!({
            "agent": {
                "default": "codew",
                "claude": {},
                "cursor": {},
                "pi": {},
                "codewhale": { "enabled": true }
            }
        });
        let upgraded = upgrade_config_json(raw);
        let config: GatewayConfig = serde_json::from_value(upgraded).unwrap();
        assert_eq!(config.agent.default, AgentProvider::Claude);
    }

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
            config
                .agent
                .profile_for(&crate::config::model::AgentProvider::Claude)
                .unwrap()
                .default_args
                .as_deref(),
            Some("--foo")
        );
    }

    #[test]
    fn save_writes_config_json() {
        let env = TestEnv::new();
        let config_path = env.home().join(".cc-gateway").join("config.json");
        assert!(!config_path.is_file());
        ConfigLoader::save(&GatewayConfig::runtime_defaults()).unwrap();
        assert!(config_path.is_file());
    }

    #[test]
    fn load_does_not_create_config_json() {
        let env = TestEnv::new();
        let config_path = env.home().join(".cc-gateway").join("config.json");
        assert!(!config_path.is_file());

        let config = ConfigLoader::load().unwrap();
        assert!(!config_path.is_file());
        assert!(
            !config
                .agent
                .profile_for(&crate::config::model::AgentProvider::Claude)
                .unwrap()
                .enabled
        );
        assert!(ConfigLoader::initialize_runtime().is_ok());
    }

    #[test]
    fn upgrade_moves_legacy_platform_keys_under_platforms() {
        let raw = json!({
            "feishu": { "enabled": true, "app_id": "x" },
            "telegram": { "enabled": false }
        });
        let upgraded = upgrade_config_json(raw);
        let config: GatewayConfig = serde_json::from_value(upgraded).unwrap();
        assert!(config.platforms.feishu.enabled);
        assert_eq!(config.platforms.feishu.app_id, "x");
        assert!(!config.platforms.telegram.enabled);
    }

    #[test]
    fn load_from_rejects_unknown_agent_profile_key() {
        let env = TestEnv::new();
        let config_path = env.home().join(".cc-gateway").join("config.json");
        std::fs::write(
            &config_path,
            r#"{"agent":{"default":"claude","claude":{},"not_a_provider":{}}}"#,
        )
        .unwrap();
        let err = ConfigLoader::load_from(&config_path).unwrap_err();
        assert!(err.to_string().contains("not_a_provider"));
    }

    #[test]
    fn load_from_persists_platform_migration() {
        let env = TestEnv::new();
        let path = env.home().join(".cc-gateway").join("config.json");
        std::fs::write(
            &path,
            r#"{
                "feishu": { "enabled": true, "app_id": "x" },
                "agent": { "default": "claude", "claude": { "enabled": true } }
            }"#,
        )
        .unwrap();

        ConfigLoader::load_from(&path).expect("load should migrate and persist");

        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(on_disk.get("platforms").is_some());
        assert!(on_disk.get("feishu").is_none());
        let providers = on_disk
            .get("agent")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.as_object())
            .expect("agent.providers object");
        assert!(providers.contains_key("opencode"));
        assert!(providers.contains_key("kimi"));
        assert!(providers.contains_key("gemini"));
        assert!(providers.contains_key("pi"));
        assert!(!on_disk
            .get("agent")
            .and_then(|v| v.as_object())
            .expect("agent")
            .contains_key("claude"));
    }

    #[test]
    fn load_from_persists_missing_registry_agent_profiles() {
        let env = TestEnv::new();
        let path = env.home().join(".cc-gateway").join("config.json");
        std::fs::write(
            &path,
            r#"{"agent":{"default":"claude","claude":{"enabled":false}}}"#,
        )
        .unwrap();

        ConfigLoader::load_from(&path).expect("load should normalize agent profiles");

        let on_disk: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let providers = on_disk
            .get("agent")
            .and_then(|v| v.get("providers"))
            .and_then(|v| v.as_object())
            .expect("agent.providers object");
        for id in ["cursor", "pi", "opencode", "kimi", "gemini"] {
            assert!(
                providers.contains_key(id),
                "normalized config should include {id} on disk"
            );
        }
    }

    #[test]
    fn load_from_skips_write_when_already_canonical() {
        let env = TestEnv::new();
        let path = env.home().join(".cc-gateway").join("config.json");
        let canonical = GatewayConfig::default();
        ConfigLoader::save_to(&path, &canonical).unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let mtime_before = std::fs::metadata(&path).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        ConfigLoader::load_from(&path).expect("canonical load");
        let after = std::fs::read_to_string(&path).unwrap();
        let mtime_after = std::fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(before, after, "canonical config should not be rewritten");
        assert_eq!(mtime_before, mtime_after);
    }

    #[test]
    fn upgrade_migrates_flat_provider_keys_under_providers() {
        let raw = json!({
            "agent": {
                "default": "claude",
                "claude": { "enabled": true },
                "cursor": { "enabled": false }
            }
        });
        let upgraded = upgrade_config_json(raw);
        let agent = upgraded
            .get("agent")
            .and_then(|v| v.as_object())
            .expect("agent");
        assert!(!agent.contains_key("claude"));
        let providers = agent
            .get("providers")
            .and_then(|v| v.as_object())
            .expect("providers");
        assert_eq!(
            providers
                .get("claude")
                .and_then(|v| v.get("enabled"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            providers
                .get("cursor")
                .and_then(|v| v.get("enabled"))
                .and_then(|v| v.as_bool()),
            Some(false)
        );
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
        let effective = config.agent.effective_config().unwrap();
        assert_eq!(effective.provider, AgentProvider::Cursor);
        assert_eq!(effective.cli_path, "agent");
    }
}
