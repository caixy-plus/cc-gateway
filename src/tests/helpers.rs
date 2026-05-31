use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

use crate::config::model::{AgentProfiles, FeishuConfig};
use crate::platform::feishu::FeishuPlatform;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub(crate) struct TestEnv {
    _lock: MutexGuard<'static, ()>,
    previous_home: Option<String>,
    previous_userprofile: Option<String>,
    previous_path: Option<String>,
    root: tempfile::TempDir,
}

impl TestEnv {
    pub(crate) fn new() -> Self {
        let lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let root = tempfile::tempdir_in(std::env::current_dir().unwrap())
            .expect("test temp dir should be created in workspace");
        let previous_home = std::env::var("HOME").ok();
        let previous_userprofile = std::env::var("USERPROFILE").ok();
        let previous_path = std::env::var("PATH").ok();
        std::env::set_var("HOME", root.path());
        std::env::set_var("USERPROFILE", root.path());
        // Prepend test home to PATH so resolve_cli_path can find fake binaries
        let new_path = format!(
            "{}:{}",
            root.path().display(),
            previous_path.as_deref().unwrap_or("")
        );
        std::env::set_var("PATH", &new_path);
        std::fs::create_dir_all(root.path().join(".cc-gateway")).unwrap();
        GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
        Self {
            _lock: lock,
            previous_home,
            previous_userprofile,
            previous_path,
            root,
        }
    }

    pub(crate) fn home(&self) -> &Path {
        self.root.path()
    }

    pub(crate) fn fake_agent_profiles(&self) -> AgentProfiles {
        let mut profiles = AgentProfiles::default();
        create_fake_agent_cli(self.home());
        profiles.claude.default_args = Some(String::new());
        profiles
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
        if let Some(home) = self.previous_home.as_deref() {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(path) = self.previous_path.as_deref() {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(userprofile) = self.previous_userprofile.as_deref() {
            std::env::set_var("USERPROFILE", userprofile);
        } else {
            std::env::remove_var("USERPROFILE");
        }
    }
}

#[cfg(not(windows))]
pub(crate) fn create_fake_agent_cli(home: &Path) -> PathBuf {
    let script = home.join("claude");
    std::fs::write(
        &script,
        r#"#!/bin/sh
session_id="fake-session"
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--resume" ]; then
    shift
    session_id="$1"
  fi
  shift || true
done
mkdir -p "$HOME/.claude/sessions"
printf '{"sessionId":"%s"}\n' "$session_id" > "$HOME/.claude/sessions/$$.json"
while IFS= read -r line; do
  printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"fake reply"}]}}\n'
  printf '{"type":"result","result":"fake reply","usage":{"input_tokens":1,"output_tokens":2}}\n'
done
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

#[cfg(windows)]
pub(crate) fn create_fake_agent_cli(home: &Path) -> PathBuf {
    let ps1 = home.join("claude.ps1");
    std::fs::write(
        &ps1,
        r#"$sessionId = "fake-session"
for ($i = 0; $i -lt $args.Count; $i++) {
  if ($args[$i] -eq "--resume" -and ($i + 1) -lt $args.Count) {
    $sessionId = $args[$i + 1]
    $i++
  }
}
$sessionDir = Join-Path $env:USERPROFILE ".claude\sessions"
New-Item -ItemType Directory -Force -Path $sessionDir | Out-Null
Set-Content -LiteralPath (Join-Path $sessionDir "$PID.json") -Value "{`"sessionId`":`"$sessionId`"}" -NoNewline -Encoding UTF8
while (($line = [Console]::In.ReadLine()) -ne $null) {
  Write-Output '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"fake reply"}]}}'
  Write-Output '{"type":"result","result":"fake reply","usage":{"input_tokens":1,"output_tokens":2}}'
}
"#,
    )
    .unwrap();

    let cmd = home.join("fake-claude.cmd");
    std::fs::write(
        &cmd,
        format!(
            "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" %*\r\n",
            ps1.display()
        ),
    )
    .unwrap();
    cmd
}

pub(crate) fn feishu_platform(default_dir: &str) -> FeishuPlatform {
    FeishuPlatform::new(
        FeishuConfig {
            enabled: true,
            app_id: "app-id".to_string(),
            app_secret: "app-secret".to_string(),
            require_pairing: false,
        },
        default_dir,
        AgentProfiles::default(),
        false,
    )
}

pub(crate) fn feishu_text_event(
    message_id: &str,
    chat_id: &str,
    chat_type: &str,
    sender_open_id: &str,
    text: &str,
) -> serde_json::Value {
    serde_json::json!({
        "header": {
            "event_type": "im.message.receive_v1"
        },
        "event": {
            "sender": {
                "sender_id": {
                    "open_id": sender_open_id
                }
            },
            "message": {
                "message_id": message_id,
                "message_type": "text",
                "chat_id": chat_id,
                "chat_type": chat_type,
                "content": serde_json::json!({"text": text}).to_string()
            }
        }
    })
}
