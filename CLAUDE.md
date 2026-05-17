# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cc-gateway is a Rust gateway that exposes local Claude Code sessions to remote users via a Feishu (Lark) bot and an interactive local CLI. It spawns Claude Code as a subprocess, communicates over stdin/stdout using the `stream-json` protocol, and bridges messages between Claude and external interfaces.

## Build & Test

```bash
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

- **`daemon/mod.rs`**: PID-file-based daemon management. `start()` spawns a detached child running `cc-gateway _daemon`. `stop()` sends SIGTERM (Unix) or `taskkill` (Windows). `run()` loads config, writes PID file, and starts `DaemonEngine`.
- **`daemon/engine.rs`**: Core async engine. Creates `ClaudeController` + `CommandRouter`, optionally starts `FeishuPlatform`, then waits for shutdown signal (SIGTERM/SIGINT).

### Claude Session (`src/claude/`)

- **`claude/session.rs`**: Spawns the `claude` subprocess with `--input-format stream-json --output-format stream-json --permission-prompt-tool stdio` plus any `default_args` from config. Reads stdout line-by-line, deserializes into `OutputEvent`, and sends them over an async channel. Writes `InputMessage` JSON lines to stdin. Extra args from `/claude <args>` are appended after config defaults.
- **`claude/protocol.rs`**: Defines the `stream-json` protocol types. `InputMessage` (user messages, permission responses) and `OutputEvent` (system, assistant, result, control_request, error). Key helper: `is_permission_request()` detects tool permission requests.
- **`claude/controller.rs`**: Owns the active `ClaudeSession`, exposes `start_session`, `stop_session`, `send_message`. Emits `ControllerEvent` (Text, Thinking, ToolUse, ToolResult, PermissionRequest, Error, Done) over an async channel. Manages `work_dir` and `pending_permission` state. Validates all paths are under the user's home directory via `ensure_under_home`.

### Command Routing (`src/command/`)

- **`command/router.rs`**: First line of message handling. When a Claude session is active, only `/quit` is handled locally; everything else is forwarded directly to Claude. When inactive, checks builtins (`/help`, `/cd`, `/claude`, `/pwd`, `/ll`, `/quit`) first, then forwards regular text to Claude. Returns `Some(response)` for immediate replies, `None` when the message was sent to Claude (responses come via the event channel).
- **`command/builtin.rs`**: Implements gateway commands: `/help`, `/cd`, `/claude`, `/pwd`, `/ll`, `/quit`. `/cd` canonicalizes paths and restarts the session. `/ll` uses `crossterm` for an interactive TUI directory picker (directory-only, `/` suffix, Enter changes directory without starting a session).
- **`command/forward.rs`**: Forwards regular text as user messages to Claude. Returns an error prompt if no session is active.

### Feishu Platform (`src/platform/`)

- **`platform/feishu.rs`**: WebSocket client for Feishu's pbbp2 protocol (protobuf frames). Gets tenant access token, connects to WS endpoint, handles heartbeats, deduplicates messages, normalizes events into `NormalizedMessage`, routes through `CommandRouter`, and polls `ClaudeController` events to reply back.
  - `/ll` in Feishu is intercepted before routing: sends an interactive card listing folders from `feishu.default_dir`. Card buttons carry `value: { "cmd": "cd", "path": "...", "chat_id": "..." }`.
  - `/cd` in Feishu is intercepted before routing: resolves and canonicalizes the path, enforces that the result stays within `feishu.default_dir`, then calls `set_work_dir`.
  - Card callbacks with `cmd == "cd"` are handled directly: call `controller.init_work_dir(path)` and reply with confirmation text.
  - Unknown slash commands when no session is active receive "Unknown command. Available commands: /help, /cd, /claude, /ll, /quit".
- **`platform/proto/mod.rs`**: Protobuf frame codec for Feishu pbbp2 (METHOD_CONTROL / METHOD_DATA, SERVICE_IM / SERVICE_CARD).

### Configuration (`src/config/`)

- **`config/loader.rs`**: Loads `~/.cc-gateway/config.json` with `${VAR}` environment variable substitution.
- **`config/model.rs`**: `GatewayConfig` with `log`, `claude`, `feishu` sections. `ClaudeConfig` has `cli_path` and `default_args`. `FeishuConfig` has `default_dir`. Defaults are defined here.
- **`config/wizard.rs`**: Interactive config editor invoked by `cc-gateway config`. Prompts for log, claude, and feishu settings.

### Skills (`src/skill/`)

- **`skill/mod.rs`**: Scans `~/.cc-gateway/skills/` and `.claude/skills/` for `.md` files with optional YAML frontmatter. Loaded skills can be injected as the system prompt via `/skill <name>`.

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
- **Event channels**: `ClaudeController` uses an `mpsc::unbounded_channel` to decouple the stdout reader from the consumer (CLI or Feishu). Consumers poll `recv_event()`.
- **Detached daemon**: The daemon is a separate OS process. `start()` spawns `cc-gateway _daemon` with stdin/stdout/stderr nulled and a new process group (Unix).
- **Config dir**: `~/.cc-gateway/` holds `config.json`, `daemon.pid`, `logs/`, and `skills/`.
