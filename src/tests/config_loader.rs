use crate::config::loader::ConfigLoader;
use std::env;
use std::fs;
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
        None => {
            let _ = env::remove_var("HOME");
        }
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
        "agent": {
            "default": "claude",
            "claude": {
                "cli_path": "claude",
                "default_args": "--dangerously-skip-permissions"
            },
            "cursor": {}
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
    assert_eq!(
        config.agent.claude.cli_path.as_deref(),
        Some("claude")
    );
    assert!(!config.feishu.enabled);
    assert_eq!(config.default_dir, "~/TestWorkspace");
}

#[test]
fn test_load_from_env_var_substitution() {
    env::set_var("CCG_TEST_KEY", "my-secret-key");
    env::set_var("CCG_TEST_DIR", "/substituted/dir");

    let tmp_path =
        std::env::temp_dir().join(format!("cc-gateway-cfg-env-{}.json", std::process::id()));
    let json = r#"{
        "log": {
            "level": "info",
            "file": "${CCG_TEST_DIR}/gateway.log"
        },
        "agent": {
            "default": "claude",
            "claude": { "cli_path": "claude", "default_args": "" },
            "cursor": {}
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

#[test]
fn load_from_upgrades_legacy_top_level_claude_block() {
    let tmp_path = std::env::temp_dir().join(format!("cc-gateway-migrate-{}.json", std::process::id()));
    let json = r#"{
        "claude": {
            "cli_path": "legacy-claude",
            "default_args": "--legacy"
        }
    }"#;
    {
        let mut file = fs::File::create(&tmp_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();
    }
    let config = ConfigLoader::load_from(&tmp_path).unwrap();
    let _ = fs::remove_file(&tmp_path);

    assert_eq!(
        config.agent.claude.cli_path.as_deref(),
        Some("legacy-claude")
    );
    assert_eq!(config.agent.claude.default_args.as_deref(), Some("--legacy"));
}
