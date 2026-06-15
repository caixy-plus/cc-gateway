use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use crate::config::model::AgentProfiles;
use crate::session::channel_manager::GLOBAL_CHANNEL_SESSIONS;
use anyhow::Result;

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// Start a history recorder subscriber for the current Tokio runtime.
///
/// Each `#[tokio::test]` gets a fresh runtime; a process-wide "already started" flag would
/// leave later tests without a live recorder after the first runtime is dropped.
fn ensure_history_recorder_started() {
    if tokio::runtime::Handle::try_current().is_ok() {
        crate::history::recorder::start_recorder();
    }
}

/// Poll until the gateway history JSONL exists (background recorder writes asynchronously).
pub(crate) async fn wait_for_gateway_history(path: &Path) -> Result<()> {
    for _ in 0..200 {
        if path.is_file() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    anyhow::bail!("gateway history file should exist: {}", path.display())
}

/// Wait until the background history recorder has written the JSONL file.
pub(crate) async fn ensure_gateway_history(path: &Path) -> Result<()> {
    wait_for_gateway_history(path).await
}

pub(crate) struct TestEnv {
    _lock: MutexGuard<'static, ()>,
    previous_home: Option<String>,
    previous_userprofile: Option<String>,
    previous_path: Option<String>,
    _root: tempfile::TempDir,
    home: PathBuf,
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
        ensure_history_recorder_started();
        let home = root.path().to_path_buf();
        Self {
            _lock: lock,
            previous_home,
            previous_userprofile,
            previous_path,
            _root: root,
            home,
        }
    }

    /// Use `$CARGO_MANIFEST_DIR` as HOME and ensure `./test_work_dir` exists at repo root.
    pub(crate) fn new_with_repo_work_dir() -> Self {
        let lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let work_dir = manifest.join("test_work_dir");
        std::fs::create_dir_all(&work_dir).expect("create repo test_work_dir");
        std::fs::create_dir_all(manifest.join(".cc-gateway"))
            .expect("create .cc-gateway under repo");

        let previous_home = std::env::var("HOME").ok();
        let previous_userprofile = std::env::var("USERPROFILE").ok();
        let previous_path = std::env::var("PATH").ok();
        std::env::set_var("HOME", &manifest);
        std::env::set_var("USERPROFILE", &manifest);
        // Isolate from the developer's real `claude` on PATH — only the fake script in the repo root.
        std::env::set_var("PATH", manifest.as_os_str());
        GLOBAL_CHANNEL_SESSIONS.reset_for_tests();
        ensure_history_recorder_started();
        let fake_claude = create_fake_agent_cli(&manifest);
        let _ = fake_claude;

        let root = tempfile::tempdir_in(&manifest).expect("scratch temp dir for TestEnv");
        Self {
            _lock: lock,
            previous_home,
            previous_userprofile,
            previous_path,
            _root: root,
            home: manifest,
        }
    }

    pub(crate) fn repo_work_dir(&self) -> PathBuf {
        self.home.join("test_work_dir")
    }

    pub(crate) fn home(&self) -> &Path {
        &self.home
    }

    pub(crate) fn fake_agent_profiles(&self) -> AgentProfiles {
        let mut profiles = AgentProfiles::default();
        create_fake_agent_cli(self.home());
        profiles
            .profile_mut(&crate::config::model::AgentProvider::Claude)
            .default_args = Some(String::new());
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
resume_used="none"
memory_file="$HOME/.cc-gateway/.test_claude_memory"
while [ "$#" -gt 0 ]; do
  if [ "$1" = "--resume" ]; then
    shift
    session_id="$1"
    resume_used="$1"
  elif [ "$1" = "--mcp-config" ]; then
    shift
    printf '%s' "$1" > "$HOME/.cc-gateway/.test_last_mcp_config"
  elif [ "$1" = "--model" ]; then
    shift
    mkdir -p "$HOME/.cc-gateway"
    printf '%s' "$1" > "$HOME/.cc-gateway/.test_claude_model"
    # Simulate Claude exiting at startup when asked for an entitlement-gated model
    # (e.g. the 1M-context tier the account lacks): such a launch dies immediately.
    case "$1" in
      *'[1m]') exit 1 ;;
    esac
  fi
  shift || true
done
mkdir -p "$HOME/.cc-gateway"
printf '%s' "$resume_used" > "$HOME/.cc-gateway/.test_last_resume"
mkdir -p "$HOME/.claude/sessions"
printf '{"sessionId":"%s"}\n' "$session_id" > "$HOME/.claude/sessions/$$.json"
recall=""
agent_id="${CC_GATEWAY_TEST_AGENT_SESSION_ID:-}"
if [ -z "$agent_id" ] && [ -f "$HOME/.cc-gateway/.test_agent_session_id" ]; then
  agent_id=$(cat "$HOME/.cc-gateway/.test_agent_session_id")
fi
if [ "$resume_used" != "none" ]; then
  if [ -f "$memory_file" ]; then
    recall=$(cat "$memory_file")
  fi
  if [ -n "$agent_id" ]; then
    hist="$HOME/.cc-gateway/history/${agent_id}.jsonl"
    if [ -s "$hist" ]; then
      recall=$(tail -1 "$hist")
    fi
  fi
fi
while IFS= read -r line; do
  if [ "$resume_used" != "none" ] && [ -n "$agent_id" ]; then
    hist="$HOME/.cc-gateway/history/${agent_id}.jsonl"
    if [ -s "$hist" ]; then
      recall=$(tail -1 "$hist")
    fi
  fi
  case "$line" in
    *'"type":"interrupt"'*|*'"type": "interrupt"'*)
      ;;
    *'"type":"control_request"'*|*'"type": "control_request"'*)
      # `/stop` interrupt control frame — record it, not a user turn.
      printf '%s' "$line" > "$HOME/.cc-gateway/.test_claude_stop"
      ;;
    *)
      if [ -n "$line" ]; then
        printf '%s' "$line" > "$memory_file"
      fi
      ;;
  esac
  if [ -n "$recall" ]; then
    recall_content=$(printf '%s' "$recall" | sed -n 's/.*"content":"\([^"]*\)".*/\1/p')
    if [ -n "$recall_content" ]; then
      text="recalled: $recall_content"
    else
      text="recalled: $recall"
    fi
    recall=""
  else
    text="fake reply"
  fi
  printf '{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"%s"}]}}\n' "$text"
  printf '{"type":"result","result":"%s","usage":{"input_tokens":1,"output_tokens":2}}\n' "$text"
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

#[cfg(not(windows))]
pub(crate) fn create_fake_pi_cli(home: &Path) -> PathBuf {
    let script = home.join("pi");
    std::fs::write(
        &script,
        r#"#!/bin/sh
printf '%s\n' "$@" > "$HOME/.cc-gateway/.test_pi_argv" 2>/dev/null || true
session_file="$HOME/.cc-gateway/.test_pi_session_file"
mkdir -p "$HOME/.cc-gateway/sessions"
default="$HOME/.cc-gateway/sessions/default.jsonl"
if [ ! -f "$session_file" ]; then
  printf '%s' "$default" > "$session_file"
fi
touch "$(cat "$session_file")" 2>/dev/null || true

extract_json_str() {
  key="$1"
  line="$2"
  printf '%s' "$line" | sed -n "s/.*\"$key\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -1
}

while IFS= read -r line || [ -n "$line" ]; do
  [ -z "$line" ] && continue
  case "$line" in
    *'"type":"get_state"'*|*'"type": "get_state"'*)
      req_id=$(extract_json_str id "$line")
      sf=$(cat "$session_file")
      sid=$(basename "$sf" .jsonl)
      if [ -n "$req_id" ]; then
        printf '{"type":"response","id":"%s","command":"get_state","success":true,"data":{"sessionFile":"%s","sessionId":"%s"}}\n' "$req_id" "$sf" "$sid"
      else
        printf '{"type":"response","command":"get_state","success":true,"data":{"sessionFile":"%s","sessionId":"%s"}}\n' "$sf" "$sid"
      fi
      ;;
    *'"type":"switch_session"'*)
      path=$(extract_json_str sessionPath "$line")
      if [ -f "$HOME/.cc-gateway/.test_pi_fail_switch" ]; then
        req_id=$(extract_json_str id "$line")
        if [ -n "$req_id" ]; then
          printf '{"type":"response","id":"%s","command":"switch_session","success":false,"error":"session file missing"}\n' "$req_id"
        else
          printf '{"type":"response","command":"switch_session","success":false,"error":"session file missing"}\n'
        fi
        continue
      fi
      if [ -n "$path" ]; then
        mkdir -p "$(dirname "$path")"
        touch "$path"
        printf '%s' "$path" > "$session_file"
        printf '%s' "$path" > "$HOME/.cc-gateway/.test_last_pi_switch_session"
      fi
      req_id=$(extract_json_str id "$line")
      if [ -n "$req_id" ]; then
        printf '{"type":"response","id":"%s","command":"switch_session","success":true,"data":{"cancelled":false}}\n' "$req_id"
      else
        printf '{"type":"response","command":"switch_session","success":true,"data":{"cancelled":false}}\n'
      fi
      ;;
    *'"type":"new_session"'*)
      new="$HOME/.cc-gateway/sessions/new-$$.jsonl"
      touch "$new"
      printf '%s' "$new" > "$session_file"
      req_id=$(extract_json_str id "$line")
      if [ -n "$req_id" ]; then
        printf '{"type":"response","id":"%s","command":"new_session","success":true,"data":{"cancelled":false}}\n' "$req_id"
      else
        printf '{"type":"response","command":"new_session","success":true,"data":{"cancelled":false}}\n'
      fi
      ;;
    *'"type":"get_available_models"'*)
      req_id=$(extract_json_str id "$line")
      if [ -n "$req_id" ]; then
        printf '{"type":"response","id":"%s","command":"get_available_models","success":true,"data":{"models":[{"provider":"anthropic","id":"fake-model"}]}}\n' "$req_id"
      else
        printf '{"type":"response","command":"get_available_models","success":true,"data":{"models":[{"provider":"anthropic","id":"fake-model"}]}}\n'
      fi
      ;;
    *'"type":"abort"'*)
      req_id=$(extract_json_str id "$line")
      if [ -n "$req_id" ]; then
        printf '{"type":"response","id":"%s","command":"abort","success":true}\n' "$req_id"
      else
        printf '{"type":"response","command":"abort","success":true}\n'
      fi
      ;;
    *'"type":"prompt"'*)
      printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"fake pi reply"}}'
      printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"done","reason":"stop"}}'
      printf '%s\n' '{"type":"agent_end"}'
      ;;
  esac
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
pub(crate) fn create_fake_pi_cli(home: &Path) -> PathBuf {
    create_fake_agent_cli(home)
}

#[cfg(windows)]
pub(crate) fn create_fake_agent_cli(home: &Path) -> PathBuf {
    let claude_cmd = home.join("claude.cmd");
    std::fs::write(
        &claude_cmd,
        r#"@echo off
setlocal EnableExtensions EnableDelayedExpansion
set "session_id=fake-session"
:parse_args
if "%~1"=="" goto args_done
if /I "%~1"=="--resume" (
  set "session_id=%~2"
  shift
  shift
  goto parse_args
)
if /I "%~1"=="--model" (
  if not exist "%USERPROFILE%\.cc-gateway" mkdir "%USERPROFILE%\.cc-gateway"
  > "%USERPROFILE%\.cc-gateway\.test_claude_model" echo %~2
  shift
  shift
  goto parse_args
)
shift
goto parse_args
:args_done
if not exist "%USERPROFILE%\.claude\sessions" mkdir "%USERPROFILE%\.claude\sessions"
> "%USERPROFILE%\.claude\sessions\%RANDOM%.json" echo {"sessionId":"!session_id!"}
:read_loop
set "line="
set /p "line="
if errorlevel 1 exit /b 0
if not defined line goto read_loop
set "text=fake reply"
echo {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"!text!"}]}}
echo {"type":"result","result":"!text!","usage":{"input_tokens":1,"output_tokens":2}}
goto read_loop
"#,
    )
    .unwrap();

    let legacy_cmd = home.join("fake-claude.cmd");
    std::fs::copy(&claude_cmd, &legacy_cmd).unwrap();
    claude_cmd
}
