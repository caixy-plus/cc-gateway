# Cursor Agent Provider Handoff

## Current Branch

- Branch: `feature/cursor-agent-support`
- Pushed commit: `1805d01 feat: support Cursor agent provider`
- Uncommitted change after that commit: `src/agent/cursor_acp.rs`
  - Adds Windows `.cmd` / `.bat` launch support through `cmd /C`.
  - Adds a gated real Cursor ACP smoke test.

## What Is Already Implemented

- Added a provider-neutral agent runtime under `src/agent/`.
- Kept Claude Code support through the existing stream-json protocol.
- Added Cursor Agent CLI support through `agent acp` JSON-RPC over stdio.
- Added nested provider config support:

```json
{
  "agent": {
    "default": "claude",
    "claude": {
      "cli_path": "claude",
      "default_args": "--dangerously-skip-permissions",
      "mode": "agent",
      "permission": "prompt"
    },
    "cursor": {
      "cli_path": "C:\\Users\\volun\\AppData\\Local\\cursor-agent\\agent.cmd",
      "default_args": "",
      "mode": "agent",
      "permission": "prompt"
    }
  }
}
```

- Preserved compatibility with:
  - old `claude` config when `agent` is absent
  - old flat `agent: { "provider": "cursor", ... }` shape
- Added command-level provider selection:
  - `/agent` uses `agent.default`
  - `/agent cursor` uses `agent.cursor`
  - `/agent claude` uses `agent.claude`
  - `/claude` remains a Claude alias
- Generalized persisted session metadata:
  - `provider`
  - `provider_session_id`
  - legacy `provider_session_id` is still kept for compatibility

## Verification Already Run

These passed:

```powershell
cargo check
cargo test config_model
cargo test command_router
cargo test cursor_acp
```

Real Cursor Agent CLI smoke test also passed on this machine:

```powershell
$env:CC_GATEWAY_RUN_CURSOR_AGENT_TEST='1'
$env:CC_GATEWAY_CURSOR_AGENT_PATH='C:\Users\volun\AppData\Local\cursor-agent\agent.cmd'
cargo test real_cursor_acp_smoke_test_when_enabled -- --nocapture
```

The smoke test verified:

- `agent.cmd acp` starts
- `initialize` works
- `authenticate` works
- `session/new` returns a session id
- session stop works

## Important Notes

- On Windows, launching `agent.cmd` directly from `tokio::process::Command` is unreliable. The latest uncommitted change wraps `.cmd` / `.bat` paths with `cmd /C`.
- The local Cursor Agent path that worked was:

```text
C:\Users\volun\AppData\Local\cursor-agent\agent.cmd
```

- The Cursor Agent CLI version verified by `cmd /C` was:

```text
2026.05.24-dda726e
```

## Remaining Work Toward A Polished Version

### 1. Commit The Smoke Test Fix

First inspect and commit the uncommitted change:

```powershell
git status --short
git diff -- src/agent/cursor_acp.rs
cargo test cursor_acp
$env:CC_GATEWAY_RUN_CURSOR_AGENT_TEST='1'
$env:CC_GATEWAY_CURSOR_AGENT_PATH='C:\Users\volun\AppData\Local\cursor-agent\agent.cmd'
cargo test real_cursor_acp_smoke_test_when_enabled -- --nocapture
git add src/agent/cursor_acp.rs
git commit -m "test: add Cursor ACP smoke coverage"
git push
```

### 2. Verify Resume Uses The Original Provider

Goal: a Cursor session must resume with Cursor even if `agent.default` later changes to Claude, and vice versa.

Likely files:

- `src/session/channel_manager.rs`
- `src/session/channel_model.rs`
- `src/db/mod.rs`
- `src/web/handlers/session.rs`
- `src/platform/feishu/cards.rs`

Expected behavior:

- New sessions persist `provider`.
- Resume reads the stored `provider`.
- Resume selects the stored provider profile from `AgentSettings`.
- `provider_session_id` is passed to the correct provider.

Suggested test:

- Create a session with `provider = cursor`.
- Persist `provider_session_id`.
- Change default to Claude.
- Resume the session.
- Assert the resumed runtime still uses `cursor`.

### 3. Improve `/agent-history` Display

Current behavior reuses the old Claude history flow. It can store provider data, but the user-facing list does not clearly show which provider each session belongs to.

Improve:

- Show provider in history item labels, for example:
  - `[cursor] abc12345... (project: ..., 10 messages, last: ...)`
  - `[claude] abc12345... (project: ..., 10 messages, last: ...)`
- Keep `/claude-history` as a compatibility alias.
- Prefer `/agent-history` in help text.

Likely files:

- `src/command/builtin.rs`
- `src/platform/feishu/cards.rs`
- `src/i18n/dict.rs`

### 4. Polish Cursor Permission Handling

Current ACP handling maps `session/request_permission` into the existing permission request surface and can respond allow/reject. It should be checked against real tool calls.

Tasks:

- Trigger a Cursor tool permission request in real ACP mode.
- Verify request payload fields and labels.
- Make Feishu/WebUI/Telegram display useful tool names.
- Consider support for:
  - allow once
  - reject once
  - allow always, if ACP options expose it reliably

Likely files:

- `src/agent/cursor_acp.rs`
- `src/platform/feishu/interaction.rs`
- `src/platform/feishu/ws.rs`
- `src/web/handlers/session.rs`
- `src/platform/telegram/mod.rs`

### 5. Adapt Config Wizard And WebUI Config API

Backend config now supports nested `agent.default`, `agent.claude`, and `agent.cursor`, but the interactive config wizard and frontend may still expose Claude-only settings.

Backend files:

- `src/config/wizard.rs`
- `src/web/handlers/config.rs`
- `src/web/handlers/session.rs`

Frontend is separate:

- `../cc-gateway-webui`

Desired UI:

- Select default provider.
- Edit Claude CLI path and args.
- Edit Cursor CLI path and args.
- Edit Cursor mode and permission policy.

### 6. Real End-To-End Manual Test

After wiring config, run the daemon and test:

```text
/agent claude
hello
/quit
/agent cursor
hello
/quit
/agent-history
```

Verify:

- Claude starts and responds.
- Cursor starts and responds.
- `/agent-history` shows both providers.
- Resume works for both providers.
- History files are written using `provider_session_id`.

### 7. Optional Long-Term Cleanup

Many public types still use Claude names for compatibility:

- `AgentController`
- `AgentSession`
- `AgentEventPoller`
- `provider_session_id`
- `ActiveAgentRuntime`

This is acceptable for the current branch, but a follow-up refactor could rename these to Agent-prefixed names once behavior is stable.

## Suggested Next Order

1. Commit the current `cursor_acp.rs` smoke test fix.
2. Fix resume to always use the stored provider.
3. Add provider labels to `/agent-history`.
4. Run real `/agent cursor` and `/agent claude` manual tests.
5. Update config wizard / WebUI config surfaces.
