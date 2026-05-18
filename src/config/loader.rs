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

#[allow(dead_code)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::io::Write;
    use std::sync::Mutex;

    static HOME_LOCK: Mutex<()> = Mutex::new(());

    fn with_fake_home<T>(f: impl FnOnce(&std::path::Path) -> T) -> T {
        let _guard = HOME_LOCK.lock().unwrap();
        let original = env::var("HOME").ok();
        let fake_home = std::env::temp_dir().join(format!("cc-gateway-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&fake_home);
        fs::create_dir_all(&fake_home).unwrap();
        env::set_var("HOME", &fake_home);
        let result = f(&fake_home);
        match original {
            Some(v) => env::set_var("HOME", v),
            None => { let _ = env::remove_var("HOME"); }
        }
        let _ = fs::remove_dir_all(&fake_home);
        result
    }

    #[test]
    fn test_ensure_config_dir_creates_directory() {
        with_fake_home(|fake_home| {
            let result = ConfigLoader::ensure_config_dir();
            assert!(result.is_ok());
            let dir = result.unwrap();
            assert!(dir.exists());
            assert_eq!(dir, fake_home.join(".cc-gateway"));
        });
    }

    #[test]
    fn test_config_path_returns_expected_path() {
        with_fake_home(|fake_home| {
            let path = ConfigLoader::config_path().unwrap();
            assert_eq!(path, fake_home.join(".cc-gateway").join("config.json"));
        });
    }

    #[test]
    fn test_load_from_valid_json() {
        let tmp_path = std::env::temp_dir().join(format!("cc-gateway-cfg-{}.json", std::process::id()));
        let json = r#"{
            "log": {
                "level": "debug",
                "file": "/tmp/test.log"
            },
            "claude": {
                "cli_path": "claude",
                "default_args": "--dangerously-skip-permissions"
            },
            "feishu": {
                "enabled": false,
                "app_id": "",
                "app_secret": "",
                "allow_from": "*",
                "encrypt_key": ""
            },
            "default_dir": "~/TestWorkspace"
        }"#;
        {
            let mut file = fs::File::create(&tmp_path).unwrap();
            file.write_all(json.as_bytes()).unwrap();
        }
        let config = ConfigLoader::load_from(&tmp_path).unwrap();
        let _ = fs::remove_file(&tmp_path);
        assert_eq!(config.log.level, "debug");
        assert_eq!(config.log.file, "/tmp/test.log");
        assert_eq!(config.claude.cli_path, "claude");
        assert!(!config.feishu.enabled);
        assert_eq!(config.default_dir, "~/TestWorkspace");
    }

    #[test]
    fn test_load_from_env_var_substitution() {
        env::set_var("CCG_TEST_KEY", "my-secret-key");
        env::set_var("CCG_TEST_DIR", "/substituted/dir");

        let tmp_path = std::env::temp_dir().join(format!("cc-gateway-cfg-env-{}.json", std::process::id()));
        let json = r#"{
            "log": {
                "level": "info",
                "file": "${CCG_TEST_DIR}/gateway.log"
            },
            "claude": {
                "cli_path": "claude",
                "default_args": ""
            },
            "feishu": {
                "enabled": true,
                "app_id": "${CCG_TEST_KEY}",
                "app_secret": "${CCG_TEST_KEY}",
                "allow_from": "*",
                "encrypt_key": ""
            },
            "default_dir": "~/Workspace"
        }"#;
        {
            let mut file = fs::File::create(&tmp_path).unwrap();
            file.write_all(json.as_bytes()).unwrap();
        }
        let config = ConfigLoader::load_from(&tmp_path).unwrap();
        let _ = fs::remove_file(&tmp_path);
        assert_eq!(config.log.file, "/substituted/dir/gateway.log");
        assert_eq!(config.feishu.app_id, "my-secret-key");
        assert_eq!(config.feishu.app_secret, "my-secret-key");

        env::remove_var("CCG_TEST_KEY");
        env::remove_var("CCG_TEST_DIR");
    }
}
