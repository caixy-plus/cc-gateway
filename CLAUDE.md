# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cc-gateway is a Rust gateway that exposes local agent sessions to remote users via chat bot platforms (Feishu/Lark, Telegram) and an interactive local CLI. It spawns provider CLIs (e.g. `claude`, Cursor `agent acp`), communicates over stdin/stdout, and bridges messages between the provider and external interfaces.

## Project Structure

This is a **frontend/backend split** project:

- **Backend** (this repo): Rust gateway with Axum HTTP server. Embeds the WebUI static files via `rust-embed` (`src/web/handlers/ui.rs` → `webui/dist/`).
- **Frontend** (separate repo): React 18 + Vite + TypeScript. Lives at `../cc-gateway-webui` (or clone from `https://github.com/caixy-plus/cc-gateway-webui.git`).

Workflow: edit frontend → `npm run build` in `cc-gateway-webui` → copy `dist/` into this repo's `webui/dist/` → rebuild Rust binary.

Notes:
- The frontend repo is expected to be a **sibling directory** (one level up from this repo). If `../cc-gateway-webui` is missing, clone it first.
- If `webui/dist/` is missing (or not embedded in the Rust binary), the WebUI will show a fallback page indicating the frontend artifacts were not embedded.
- The WebUI frontend (`../cc-gateway-webui`) is an **auxiliary repo** for this project. Before cutting a release tag, ensure any WebUI changes have been **committed and pushed** in the frontend repo; the release workflow builds the frontend from that repo, so unpushed changes will not be included in the release artifacts.
- **NEVER commit `webui/dist/`** — it is gitignored (`webui/.gitignore`: `dist/`) and exists only as a **local build artifact** for local packaging/embedding. The release workflow `rm -rf`s and rebuilds it from the frontend repo, so committing it is pointless and noisy. When committing backend changes, only stage `src/`, `Cargo.toml`, etc. — never `git add -f` or otherwise force dist files. The legacy force-added dist files have been untracked; do not re-add them.
- **No manual rebuild/integration of `webui/dist/` is needed.** Do not hand-run `npm run build` + copy `dist/` just to "integrate" the frontend. Integration is automatic:
  - **Local**: `./install_local.sh` builds the frontend (`npm run build` in `../cc-gateway-webui`), refreshes `webui/dist/`, then `cargo build --release` embeds it.
  - **Release**: the CI workflow checks out and builds the frontend repo and copies it into `webui/dist/` before compiling.
  - So after editing the frontend, just commit/push the **frontend repo** and run `./install_local.sh` (local) or push a tag (release) — never commit `webui/dist/` to wire it up.

## Local Development Install

Platform-specific scripts that build from source (including the frontend) and install locally:

- **macOS / Linux**: `./install_local.sh`
- **Windows**: `powershell -ExecutionPolicy Bypass -File .\install_local.ps1`

Production install scripts (download pre-built binaries from GitHub Releases):
- **macOS / Linux**: `./install.sh`
- **Windows**: `.\install.ps1`

## Build & Test

```sh
cargo build --release     # Release build
cargo build               # Debug build
cargo test                # Run all tests
cargo test <module>       # Run tests matching name (e.g., cargo test router)
cargo run                 # Run interactive CLI mode
cargo run -- start        # Start daemon (spawns background process)
```

## Development Principles

- **Use TDD for feature work and bug fixes**: write or update a focused failing test first, implement the smallest change that makes it pass, then refactor with tests green.
- **Run tests based on change scope**: after functional changes, choose the fastest relevant test set from the touched modules and risk area instead of defaulting to full `cargo test` every time. Run full tests when changes touch shared infrastructure, cross-platform behavior, persistence, command/session lifecycle, or before final verification of broad refactors.
- **Document skipped verification**: if a change is docs-only or tests are intentionally not run, say so in the final response.
- **Release tagging must match Cargo version**: before pushing a release tag `vX.Y.Z`, ensure `Cargo.toml` `[package].version` is exactly `X.Y.Z`. The release workflow enforces this and will fail if they differ.
- **Version bump rule (project convention)**: use `MAJOR.MINOR.PATCH`.
  - `PATCH` ranges **0–9**. When it reaches **9**, the next bump rolls over to `0` and increments `MINOR`.
  - `MINOR` ranges **0–19**. When it reaches **19**, the next bump rolls over to `0` and increments `MAJOR`.
  - Example: `1.5.9` → `1.6.0`; `1.19.9` → `2.0.0`.

## Architecture

### Entry Points

- **`src/main.rs`**: CLI entry. Uses `clap` subcommands. No subcommand → interactive mode (`cli::interactive::run_interactive`). `start`/`stop`/`restart`/`log`/`status`/`enable`/`disable` manage the daemon. `_daemon` is the hidden command that actually runs the engine.
- **`src/cli/interactive.rs`**: Local REPL using `rustyline`. Spawns an async event listener that prints Claude responses with ANSI boxes, and a readline loop that feeds input into `CommandRouter`. Provides `Tab` completion and grey inline hints for `/` commands via `CommandHelper`.

### Daemon Lifecycle (`src/daemon/`)

- **`daemon/mod.rs`**: PID-file-based daemon management with triple singleton lock: port binding (configurable via `port`), `.daemon-starting.lock` for `start()` atomicity, and PID file `flock` held for daemon lifetime. `start()` spawns a detached child running `cc-gateway _daemon`. `stop()` sends SIGTERM (Unix) or `taskkill` (Windows). `run()` loads config, writes PID file, and starts `DaemonEngine`.
- **`daemon/engine.rs`**: Core async engine. Starts all enabled `Platform` integrations (`feishu.enabled`, `telegram.enabled`) concurrently, then waits for shutdown signal (SIGTERM/SIGINT). On shutdown, calls `platform.shutdown()` on each enabled platform to gracefully terminate all active chat sessions.

### Agent Runtime (`src/agent/` + `src/runtime/`)

- **`agent/session.rs`**: Provider-neutral runtime (`AgentRuntime`) that spawns either Claude Code (stream-json) or Cursor ACP (`agent acp`) based on selected provider profile.
- **`agent/cursor_acp.rs`**: Cursor ACP JSON-RPC client over stdio (initialize/auth/session/new/session/load/session/prompt).
- **`runtime/session.rs`** / **`runtime/protocol.rs`**: Claude Code stream-json protocol session/types.
- **`runtime/controller.rs`**: Owns the active `AgentRuntime`, exposes start/stop/send. Emits `ControllerEvent` (Text, Thinking, ToolUse, ToolResult, PermissionRequest, Error, Done). Manages `work_dir` and pending permission/resume state.

### Command Routing (`src/command/`)

- **`command/router.rs`**: First line of message handling. When a session is active, gateway controls (e.g. `/quit`) are handled locally; other text is forwarded to the active agent. When inactive, parses builtins (`/help`, `/cd`, `/agent`, `/agents`, `/agent-history`, `/pwd`, `/ll`, `/mkdir`, `/show-thinking`, `/hide-thinking`, `/quit`).
- **`command/builtin.rs`**: Implements gateway commands and help text.
- **`command/forward.rs`**: Forwards regular text as user messages to Claude. Returns an error prompt if no session is active.

### Platform Layer (`src/platform/`)

- **`platform/mod.rs`**: Defines the `Platform` trait (`run()` and `shutdown()`). All platform integrations implement this trait so `DaemonEngine` is platform-agnostic.
- **`platform/feishu/mod.rs`**: WebSocket client for Feishu's pbbp2 protocol (protobuf frames). Gets tenant access token, connects to WS endpoint, handles heartbeats, deduplicates messages, normalizes events into `NormalizedMessage`, routes through `CommandRouter`, and polls `AgentController` events to reply back. Each chat gets its own `ChatSession` (isolated Claude subprocess).
  - `/ll` in Feishu is intercepted before routing: sends an interactive card listing folders from `default_dir`. Card buttons carry `value: { "cmd": "cd", "path": "...", "chat_id": "..." }`.
  - `/cd` in Feishu is intercepted before routing: resolves and canonicalizes the path, enforces that the result stays within `default_dir`, then calls `set_work_dir`.
  - Card callbacks with `cmd == "cd"` are handled directly: call `controller.init_work_dir(path)` and reply with confirmation text.
  - Unknown slash commands when no session is active receive a list of available commands (see `feishu.unknown_command`).
- **`platform/telegram/mod.rs`**: Telegram Bot API integration using long-polling `getUpdates`. Each chat gets its own `TgChatSession` (isolated Claude subprocess). Routes messages through `CommandRouter` and streams Claude responses back via `sendMessage`.
- **`platform/proto/mod.rs`**: Protobuf frame codec for Feishu pbbp2 (METHOD_CONTROL / METHOD_DATA, SERVICE_IM / SERVICE_CARD).

### Platform Reference Docs

- **Feishu / Lark Open Platform**: https://open.feishu.cn/document/home/index
  - Card JSON v2.0 breaking changes: https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes
  - Button component (V2): https://open.feishu.cn/document/feishu-cards/card-json-v2-components/interactive-components/button
  - WebSocket real-time messaging (pbbp2): https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes (search "pbbp2")
- **Telegram Bot API**: https://core.telegram.org/bots/api
  - `getUpdates` long-polling reference: https://core.telegram.org/bots/api#getupdates
  - `sendMessage` reference: https://core.telegram.org/bots/api#sendmessage

### Configuration (`src/config/`)

- **`config/loader.rs`**: Loads `~/.cc-gateway/config.json` with `${VAR}` environment variable substitution.
- **`config/model.rs`**: `GatewayConfig` with `log`, `agent` (provider profiles), `feishu`, `telegram` sections, plus top-level fields like `port`, `default_dir`, `show_thinking`, `media_retention_days`.
- **`config/wizard.rs`**: Interactive config editor invoked by `cc-gateway config`.

### Web Server (`src/web/`)

- **`web/server.rs`**: Axum HTTP server bound to `config.port` (replaces the old throwaway TCP singleton listener). Serves the embedded WebUI static files and exposes REST APIs.
- **`web/handlers/ui.rs`**: Static file handler using `rust-embed` to serve the compiled frontend from `webui/dist/` at the root path.
- **`web/handlers/session.rs`**: Session APIs — `GET /api/sessions`, `POST /api/sessions`, `DELETE /api/sessions/:id`, `POST /api/sessions/:id/messages`, `GET /api/sessions/:id/history`, `GET /api/sessions/:id/events` (SSE stream for real-time messages).
- **`web/handlers/cmd.rs`**: Gateway command APIs — `POST /api/cmd/ll`, `/api/cmd/pwd`, `/api/cmd/cd`, `/api/cmd/cd_default`.
- **`web/handlers/system.rs`**: System APIs — `GET /api/config`, `POST /api/config`, `GET /api/platforms`, `GET /api/version`, `POST /api/restart`.
- **CORS**: Configured to allow `127.0.0.1` and `localhost` origins; no auth required (local-only access).

### Session Management (`src/session/`)

- **`session/manager.rs`**: `SessionManager` holds all sessions in a `DashMap<String, Session>`. WebUI sessions also have a `WebUISessionRuntime` (controller + router) stored separately. `GLOBAL_SESSIONS` is the process-wide singleton.
- **`session/model.rs`**: `Session` struct with `id`, `source` (`WebUI` / `Feishu` / `Telegram`), `platform`, `chat_id`, `title`, `work_dir`, `active`, `provider_session_id`, `created_at`.
- **Persistence**: All sessions are persisted to SQLite (`src/db/`). On daemon restart, previously active sessions are marked inactive because their Claude subprocesses are gone.

### History Recording

- Events are written to `~/.cc-gateway/history/{session_id}.jsonl`.
- A broadcast channel (`EVENT_BUS`) fans out `ControllerEvent` to both the SSE stream and the history recorder.
- Each line: `{"timestamp": "...", "role": "user|assistant|system", "content": "...", "event_type": "..."}`.

### Database (`src/db/`)

- SQLite backend storing sessions, config overrides, and runtime state. Auto-creates tables on first access.

### Update / Version Check (`src/update/`)

- Checks GitHub Releases (`caixy-plus/cc-gateway`) for newer versions. Used by the WebUI version badge and can be triggered from the sidebar.

### Provider Session ID & Resume

- Claude Code: after spawning the `claude` subprocess, cc-gateway reads `~/.claude/sessions/{pid}.json` to extract Claude's internal session id, persists it as `provider_session_id`, and uses it to resume when supported.
- Cursor ACP: attempts `session/load` using persisted `provider_session_id` when appropriate; falls back to `session/new` if the stored session id is not found.

## Testing Patterns

### Terminal UI Tests (`src/command/builtin.rs`)

The `/ll` interactive directory picker is tested without a real terminal by abstracting I/O behind a `SelectBackend` trait:

1. **Trait**: `size()`, `draw(lines)`, `read_key()` — three operations, no crossterm types in business logic.
2. **RealBackend**: Uses `crossterm` for production. Writes `\r\n` (not `\n`) to stdout because raw mode disables automatic newline translation; omitting `\r` causes staircase/diagonal output.
3. **TestBackend**: Records every `draw()` call as `Vec<Vec<String>>` (frames), and replays injected keys. Tests assert exact strings with `==`.
4. **Pure render function**: `render_file_list(items, term_width, term_height, selected) -> (lines, scroll_row)` has no side effects and is tested independently from input handling.

This pattern lets unit tests verify layout math, scroll behavior, selection state, and key navigation with plain string assertions.

## Key Patterns

- **Session switching**: `/agent` starts a session and changes the prompt to `💬 ~/Workspace ▶`. Everything except gateway controls is forwarded to the active agent. `/quit` stops the session in gateway; exits the program when no session is active.
- **Stream-json protocol**: All Claude communication is newline-delimited JSON. Each line is one event. Claude must be launched with `--input-format stream-json --output-format stream-json`.
- **Event channels**: `AgentController` uses an `mpsc::unbounded_channel` to decouple the stdout reader from the consumer (CLI or platform). Consumers poll `recv_event()`.
- **Detached daemon**: The daemon is a separate OS process. `start()` spawns `cc-gateway _daemon` with stdin/stdout/stderr nulled and a new process group (Unix).
- **Config dir**: `~/.cc-gateway/` holds `config.json`, `daemon.pid`, `logs/`, and `skills/`.

## Internationalization (i18n)

All user-facing strings must go through the translation macros in `src/i18n/dict.rs`:

- **Static text**: `crate::t!("module.key")` returns `&str`
- **Formatted text**: `crate::t_fmt!("module.key", NAME = value, ID = id)` returns `String`

### Rules
1. Never hard-code Chinese or English user-visible messages — always add a translation key.
2. Key naming: `{module}.{descriptor}` (e.g. `feishu.permission_title`, `builtin.session_started`, `telegram.shutdown_notice`).
3. Platform-specific keys use the platform prefix (`feishu.`, `telegram.`, `webui.`, `tui.`).
4. Shared / builtin keys use the `builtin.` prefix (`builtin.help`, `builtin.session_stopped`, `builtin.dir_changed`).
5. When adding a new key, provide both English and Chinese (`Language::En` / `Language::ZhCN`) entries in `dict.rs`.
6. Internal debug / tracing messages do not need translation.
