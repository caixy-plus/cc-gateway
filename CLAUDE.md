# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cc-gateway is a Rust gateway that exposes local Claude Code sessions to remote users via chat bot platforms (Feishu/Lark, Telegram) and an interactive local CLI. It spawns Claude Code as a subprocess, communicates over stdin/stdout using the `stream-json` protocol, and bridges messages between Claude and external interfaces.

## Project Structure

This is a **frontend/backend split** project:

- **Backend** (this repo): Rust gateway with Axum HTTP server. Embeds the WebUI static files via `rust-embed` (`src/web/handlers/ui.rs` → `webui/dist/`).
- **Frontend** (separate repo): React 18 + Vite + TypeScript. Lives at `../cc-gateway-webui` (or clone from `https://github.com/caixy-plus/cc-gateway-webui.git`).

Workflow: edit frontend → `npm run build` in `cc-gateway-webui` → copy `dist/` into this repo's `webui/dist/` → rebuild Rust binary.

## Local Development Install

Platform-specific scripts that build from source (including the frontend) and install locally:

- **macOS / Linux**: `./install_local.sh`
  - Builds frontend (`npm ci && npm run build` in `../cc-gateway-webui`)
  - `cargo build --release`
  - Copies binary to `~/.local/bin/cc-gateway`
  - macOS: re-signs with `codesign -s - -f`
  - Restarts the daemon (`cc-gateway restart`)
- **Windows**: `powershell -ExecutionPolicy Bypass -File .\install_local.ps1`
  - Builds frontend (`npm ci && npm run build` in `..\cc-gateway-webui`)
  - `cargo build --release`
  - Installs to `$env:LOCALAPPDATA\cc-gateway\cc-gateway.exe`
  - Adds install dir to user PATH
  - Starts the daemon (`cc-gateway start`)

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

## Architecture

### Entry Points

- **`src/main.rs`**: CLI entry. Uses `clap` subcommands. No subcommand → interactive mode (`cli::interactive::run_interactive`). `start`/`stop`/`restart`/`log`/`status`/`enable`/`disable` manage the daemon. `_daemon` is the hidden command that actually runs the engine.
- **`src/cli/interactive.rs`**: Local REPL using `rustyline`. Spawns an async event listener that prints Claude responses with ANSI boxes, and a readline loop that feeds input into `CommandRouter`. Provides `Tab` completion and grey inline hints for `/` commands via `CommandHelper`.

### Daemon Lifecycle (`src/daemon/`)

- **`daemon/mod.rs`**: PID-file-based daemon management with triple singleton lock: port binding (configurable via `port`), `.daemon-starting.lock` for `start()` atomicity, and PID file `flock` held for daemon lifetime. `start()` spawns a detached child running `cc-gateway _daemon`. `stop()` sends SIGTERM (Unix) or `taskkill` (Windows). `run()` loads config, writes PID file, and starts `DaemonEngine`.
- **`daemon/engine.rs`**: Core async engine. Starts the configured `Platform` (Feishu or Telegram) based on `config.platform`, then waits for shutdown signal (SIGTERM/SIGINT). On shutdown, calls `platform.shutdown()` to gracefully terminate all active chat sessions.

### Claude Session (`src/claude/`)

- **`claude/session.rs`**: Spawns the `claude` subprocess with `--input-format stream-json --output-format stream-json --permission-prompt-tool stdio` plus any `default_args` from config. Reads stdout line-by-line, deserializes into `OutputEvent`, and sends them over an async channel. Writes `InputMessage` JSON lines to stdin. Extra args from `/claude <args>` are appended after config defaults.
- **`claude/protocol.rs`**: Defines the `stream-json` protocol types. `InputMessage` (user messages, permission responses) and `OutputEvent` (system, assistant, result, control_request, error). Key helper: `is_permission_request()` detects tool permission requests.
- **`claude/controller.rs`**: Owns the active `ClaudeSession`, exposes `start_session`, `stop_session`, `send_message`. Emits `ControllerEvent` (Text, Thinking, ToolUse, ToolResult, PermissionRequest, Error, Done) over an async channel. Manages `work_dir` and `pending_permission` state. Validates all paths are under the user's home directory via `ensure_under_home`.

### Command Routing (`src/command/`)

- **`command/router.rs`**: First line of message handling. When a Claude session is active, only `/quit` is handled locally; everything else is forwarded directly to Claude. When inactive, checks builtins (`/help`, `/cd`, `/claude`, `/pwd`, `/ll`, `/quit`) first, then forwards regular text to Claude. Returns `Some(response)` for immediate replies, `None` when the message was sent to Claude (responses come via the event channel).
- **`command/builtin.rs`**: Implements gateway commands: `/help`, `/cd`, `/claude`, `/pwd`, `/ll`, `/quit`. `/cd` canonicalizes paths and restarts the session. `/ll` uses `crossterm` for an interactive TUI directory picker (directory-only, `/` suffix, Enter changes directory without starting a session).
- **`command/forward.rs`**: Forwards regular text as user messages to Claude. Returns an error prompt if no session is active.

### Platform Layer (`src/platform/`)

- **`platform/mod.rs`**: Defines the `Platform` trait (`run()` and `shutdown()`). All platform integrations implement this trait so `DaemonEngine` is platform-agnostic.
- **`platform/feishu/mod.rs`**: WebSocket client for Feishu's pbbp2 protocol (protobuf frames). Gets tenant access token, connects to WS endpoint, handles heartbeats, deduplicates messages, normalizes events into `NormalizedMessage`, routes through `CommandRouter`, and polls `ClaudeController` events to reply back. Each chat gets its own `ChatSession` (isolated Claude subprocess).
  - `/ll` in Feishu is intercepted before routing: sends an interactive card listing folders from `default_dir`. Card buttons carry `value: { "cmd": "cd", "path": "...", "chat_id": "..." }`.
  - `/cd` in Feishu is intercepted before routing: resolves and canonicalizes the path, enforces that the result stays within `default_dir`, then calls `set_work_dir`.
  - Card callbacks with `cmd == "cd"` are handled directly: call `controller.init_work_dir(path)` and reply with confirmation text.
  - Unknown slash commands when no session is active receive "Unknown command. Available commands: /help, /cd, /claude, /ll, /quit".
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
- **`config/model.rs`**: `GatewayConfig` with `log`, `claude`, `feishu`, `telegram` sections, plus top-level fields: `platform`, `port`, `default_dir`, `show_thinking`, `media_retention_days`.
- **`config/wizard.rs`**: Interactive config editor invoked by `cc-gateway config`. Prompts for log, claude, and platform settings.

### Skills (`src/skill/`)

- **`skill/mod.rs`**: Scans `~/.cc-gateway/skills/` and `.claude/skills/` for `.md` files with optional YAML frontmatter. Loaded skills can be injected as the system prompt via `/skill <name>`.

### Web Server (`src/web/`)

- **`web/server.rs`**: Axum HTTP server bound to `config.port` (replaces the old throwaway TCP singleton listener). Serves the embedded WebUI static files and exposes REST APIs.
- **`web/handlers/ui.rs`**: Static file handler using `rust-embed` to serve the compiled frontend from `webui/dist/` at the root path.
- **`web/handlers/session.rs`**: Session APIs — `GET /api/sessions`, `POST /api/sessions`, `DELETE /api/sessions/:id`, `POST /api/sessions/:id/messages`, `GET /api/sessions/:id/history`, `GET /api/sessions/:id/events` (SSE stream for real-time messages).
- **`web/handlers/cmd.rs`**: Gateway command APIs — `POST /api/cmd/ll`, `/api/cmd/pwd`, `/api/cmd/cd`, `/api/cmd/cd_default`.
- **`web/handlers/system.rs`**: System APIs — `GET /api/config`, `POST /api/config`, `GET /api/platforms`, `GET /api/version`, `POST /api/restart`.
- **CORS**: Configured to allow `127.0.0.1` and `localhost` origins; no auth required (local-only access).

### Session Management (`src/session/`)

- **`session/manager.rs`**: `SessionManager` holds all sessions in a `DashMap<String, Session>`. WebUI sessions also have a `WebUISessionRuntime` (controller + router) stored separately. `GLOBAL_SESSIONS` is the process-wide singleton.
- **`session/model.rs`**: `Session` struct with `id`, `source` (`WebUI` / `Feishu` / `Telegram`), `platform`, `chat_id`, `title`, `work_dir`, `active`, `claude_session_id`, `created_at`.
- **Persistence**: All sessions are persisted to SQLite (`src/db/`). On daemon restart, previously active sessions are marked inactive because their Claude subprocesses are gone.

### History Recording

- Events are written to `~/.cc-gateway/history/{session_id}.jsonl`.
- A broadcast channel (`EVENT_BUS`) fans out `ControllerEvent` to both the SSE stream and the history recorder.
- Each line: `{"timestamp": "...", "role": "user|assistant|system", "content": "...", "event_type": "..."}`.

### Database (`src/db/`)

- SQLite backend storing sessions, config overrides, and runtime state. Auto-creates tables on first access.

### Update / Version Check (`src/update/`)

- Checks GitHub Releases (`caixy-plus/cc-gateway`) for newer versions. Used by the WebUI version badge and can be triggered from the sidebar.

### Claude Session ID & Resume (`src/claude/session.rs`)

- After spawning the `claude` subprocess, the code reads `~/.claude/sessions/{pid}.json` with retries to extract Claude's internal session ID.
- This ID is stored in `Session.claude_session_id` and passed back via `--resume` on the next `/claude` invocation so Claude Code resumes the same conversation.

## Testing Patterns

### Terminal UI Tests (`src/command/builtin.rs`)

The `/ll` interactive directory picker is tested without a real terminal by abstracting I/O behind a `SelectBackend` trait:

1. **Trait**: `size()`, `draw(lines)`, `read_key()` — three operations, no crossterm types in business logic.
2. **RealBackend**: Uses `crossterm` for production. Writes `\r\n` (not `\n`) to stdout because raw mode disables automatic newline translation; omitting `\r` causes staircase/diagonal output.
3. **TestBackend**: Records every `draw()` call as `Vec<Vec<String>>` (frames), and replays injected keys. Tests assert exact strings with `==`.
4. **Pure render function**: `render_file_list(items, term_width, term_height, selected) -> (lines, scroll_row)` has no side effects and is tested independently from input handling.

This pattern lets unit tests verify layout math, scroll behavior, selection state, and key navigation with plain string assertions.

## Key Patterns

- **Session switching**: `/claude` starts a session and changes the prompt to `💬 ~/Workspace ▶`. Everything except `/quit` is forwarded to Claude. `/quit` stops the session in gateway; exits the program when no session is active.
- **Stream-json protocol**: All Claude communication is newline-delimited JSON. Each line is one event. Claude must be launched with `--input-format stream-json --output-format stream-json`.
- **Event channels**: `ClaudeController` uses an `mpsc::unbounded_channel` to decouple the stdout reader from the consumer (CLI or platform). Consumers poll `recv_event()`.
- **Detached daemon**: The daemon is a separate OS process. `start()` spawns `cc-gateway _daemon` with stdin/stdout/stderr nulled and a new process group (Unix).
- **Config dir**: `~/.cc-gateway/` holds `config.json`, `daemon.pid`, `logs/`, and `skills/`.
