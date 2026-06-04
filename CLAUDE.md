# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cc-gateway is a Rust gateway that exposes local agent sessions to remote users via chat bot platforms (Feishu/Lark, Telegram, QQ) and WebUI. It spawns provider CLIs (e.g. `claude`, Cursor `agent acp`, `opencode acp`), communicates over stdin/stdout, and bridges messages between the provider and external interfaces.

## Project Structure

This is a **frontend/backend split** project:

- **Backend** (this repo): Rust **library** (`src/lib.rs`, crate `cc_gateway`) + thin binary (`src/main.rs`). Layered modules:

```
src/
├── main.rs              # CLI entry → cc_gateway::
├── lib.rs               # layers + crate-root re-exports (config, web, db, …)
├── core/                # agent, command, config, history, prompt, runtime, session
├── api/web/             # Axum HTTP + WebUI handlers
├── database.rs          # SQLite (crate alias `db`)
├── platform/            # Feishu, Telegram, QQ
├── daemon/              # PID, engine, lifecycle
├── utils/               # env helpers + i18n
├── types.rs             # shared type re-exports
├── update.rs, uninstall.rs
└── tests/               # integration tests (#[cfg(test)] in lib)
```

- Embeds WebUI static files via `rust-embed` (`src/api/web/handlers/ui.rs` → `webui/dist/`).
- Internal code may use `crate::config::…` via `lib.rs` re-exports; prefer `crate::core::config::…` in new code.
- **Frontend** (separate repo): React 18 + Vite + TypeScript. Lives at `../cc-gateway-webui` (or clone from `https://github.com/caixy-plus/cc-gateway-webui.git`).

Workflow: edit frontend → `npm run build` in `cc-gateway-webui` → copy `dist/` into this repo's `webui/dist/` → rebuild Rust binary.

Notes:
- The frontend repo is expected to be a **sibling directory** (one level up from this repo). If `../cc-gateway-webui` is missing, clone it first.
- If `webui/dist/` is missing (or not embedded in the Rust binary), the WebUI will show a fallback page indicating the frontend artifacts were not embedded.
- The WebUI frontend (`../cc-gateway-webui`) is an **auxiliary repo** for this project. Before cutting a release tag, follow [docs/release.md](docs/release.md) (or [docs/release.zh-CN.md](docs/release.zh-CN.md)): **commit and push webui `main` first**, then tag the backend. CI builds WebUI from GitHub, not from local `webui/dist/` — unpushed frontend changes are missing from release binaries (backend-only tag = old UI + new API).
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
cargo run -- start        # Start daemon (spawns background process)
cargo run -- webui        # Open WebUI (requires built/embedded frontend for full UI)
```

## Development Principles

- **Response language (AI assistants in this repo)**: Write final summaries, explanations, PR descriptions, and handoff messages in the **same language as the user’s initial request** that states the task or change (e.g. a bug report or feature ask). You may think and draft internally in English, but the user-visible conclusion must not switch languages unless the user does. Infer language from that first substantive message; if it is mixed or unclear, default to **Chinese (简体中文)**. This rule applies to assistant ↔ user communication only—not to product UI copy (see [Internationalization](#internationalization-i18n)).
- **No autonomous git or release actions**: Do **not** commit, push, open PRs, bump `Cargo.toml` version, push tags, run release/install scripts to publish, or create or edit GitHub Releases unless the user **explicitly asks** in the current thread (e.g. “commit”, “push”, “发版”, “打 tag”). Finishing code or tests is not permission to ship. If shipping seems appropriate, list the exact commands or steps and wait for confirmation.
- **Use TDD for feature work and bug fixes**: write or update a focused failing test first, implement the smallest change that makes it pass, then refactor with tests green.
- **Run tests based on change scope**: after functional changes, choose the fastest relevant test set from the touched modules and risk area instead of defaulting to full `cargo test` every time. Run full tests when changes touch shared infrastructure, cross-platform behavior, persistence, command/session lifecycle, or before final verification of broad refactors.
- **Document skipped verification**: if a change is docs-only or tests are intentionally not run, say so in the final response.
- **Release process (read before tagging)**: [docs/release.md](docs/release.md) / [docs/release.zh-CN.md](docs/release.zh-CN.md). **Critical:** CI embeds WebUI from **`caixy-plus/cc-gateway-webui` `main` on GitHub**, not from local `webui/dist/` or unpushed laptop changes — **commit and push the frontend repo before** pushing backend tag `vX.Y.Z`. Run `./scripts/check-release-ready.sh` from the backend repo root to fail fast if webui is dirty or unpushed.
- **Release tagging must match Cargo version** (only when the user requests a release): before pushing a release tag `vX.Y.Z`, ensure `Cargo.toml` `[package].version` is exactly `X.Y.Z`. The release workflow enforces this and will fail if they differ.
- **Version bump rule (project convention)**: use `MAJOR.MINOR.PATCH`.
  - `PATCH` ranges **0–9**. When it reaches **9**, the next bump rolls over to `0` and increments `MINOR`.
  - `MINOR` ranges **0–19**. When it reaches **19**, the next bump rolls over to `0` and increments `MAJOR`.
  - Example: `1.5.9` → `1.6.0`; `1.19.9` → `2.0.0`.
- **Release notes must be bilingual** (only when the user requests a release): when creating a GitHub Release (or editing one), write release notes with each bullet in both Chinese and English, separated by ` / `. Format: `- **中文描述** / English description — 中文细节 / English details.` This applies to both manually created and CI-created releases. If CI creates the release with auto-generated notes, edit it afterwards via `gh release edit`. Never leave only the auto-generated "Full Changelog" link as the sole body — the WebUI shows release notes directly to users, and empty notes waste the update-check feature.
- **Update user docs with the code**: adding or materially changing an **agent provider** or **chat platform** is not complete until the [user-facing documentation](#user-facing-documentation-keep-in-sync) checklist below is satisfied (English + Chinese where paired files exist). Do not ship integration-only PRs without the matching `docs/` and README updates.
- **Chat platform integration**: follow [docs/platform-integration-checklist.md](docs/platform-integration-checklist.md) (feature parity matrix + A–E checklist). Copy into PRs; check every required row.

## User-facing documentation (keep in sync)

Treat documentation as part of the feature. When reviewers (or release prep) grep for a new `id`, it should appear in setup guides—not only in Rust/WebUI code.

### New agent provider

| File | What to update |
|------|----------------|
| `docs/config.md` / `docs/config.zh-CN.md` | `agent` fields, example JSON, defaults; link to provider CLI install if non-obvious |
| `README.md` / `README.zh-CN.md` | Provider name in features / gateway-command provider list / quick start when behavior differs |
| `CLAUDE.md` | § [Adding a New Agent Provider](#adding-a-new-agent-provider) — refresh “Current providers” line; optional note under Agent Runtime if protocol is new |
| `src/utils/i18n/dict.rs` | Provider-specific user strings (see i18n rules below) |

No `docs/bots/` change unless the provider is only relevant on one platform (unusual).

### New chat platform (bot channel)

| File | What to update |
|------|----------------|
| `docs/bots/<platform>.md` / `docs/bots/<platform>.zh-CN.md` | **Create** setup guide: developer console steps, `config.json` fields, pairing, transport, UX (`/ll`, @ rules), **whether MCP `send_file` is supported**, troubleshooting, official API links |
| `docs/bots/README.md` / `docs/bots/README.zh-CN.md` | Add row to the platform table; mention pairing if applicable |
| `docs/config.md` / `docs/config.zh-CN.md` | New `GatewayConfig` section, field table, example JSON, restart vs live fields; link to `docs/bots/<platform>` |
| `docs/usage.md` / `docs/usage.zh-CN.md` | Usage section for that platform (how to talk to the bot, command quirks) |
| `README.md` / `README.zh-CN.md` | Features, architecture line, quick-start platform table, documentation index table |
| `scripts/install-docs.sh` / `scripts/install-docs.ps1` | Add EN + zh-CN URL lines (install scripts source these; do not duplicate URLs in `install.sh` / `install.ps1`) |
| `CLAUDE.md` | Project Overview; Platform layer; § [Adding a New Chat Platform](#adding-a-new-chat-platform-bot); **§ [Platform Reference Docs](#platform-reference-docs)** — add official vendor URLs (required, see that section) |
| `../cc-gateway-webui` | Settings / pairing / session source labels (see platform frontend checklist in that section) |

### Conventions

- **Bilingual pairs**: every new `docs/foo.md` user guide should have `docs/foo.zh-CN.md` (or live under `docs/bots/*.zh-CN.md`). README uses `README.md` + `README.zh-CN.md` with language links at the top.
- **Single source for setup steps**: long console walkthroughs live in `docs/bots/<platform>.md`; `docs/config.md` and README only summarize fields and link there.
- **Install output**: `install.sh` / `install.ps1` / `install_local.*` call `scripts/install-docs.*` — extend those scripts when adding a platform so fresh installs list the new guide.

## Architecture

### Entry Points

- **`src/main.rs`**: Binary entry. No subcommand → prints help (same as `--help`, exit 0). Subcommands: `start`/`stop`/`restart`/`log`/`status`/`enable`/`disable`/`init`/`webui`/…; hidden `_daemon` runs `DaemonEngine`.

### Daemon Lifecycle (`src/daemon/`)

- **`daemon.rs`**: PID-file-based daemon management with triple singleton lock: port binding (configurable via `port`), `.daemon-starting.lock` for `start()` atomicity, and PID file `flock` held for daemon lifetime. `start()` spawns a detached child running `cc-gateway _daemon`. `stop()` sends SIGTERM (Unix) or `taskkill` (Windows). `run()` loads config, writes PID file, and starts `DaemonEngine`.
- **`daemon/engine.rs`**: Core async engine. Starts all enabled `Platform` integrations (`feishu.enabled`, `telegram.enabled`, `qq.enabled`) concurrently, then waits for shutdown signal (SIGTERM/SIGINT). On shutdown, calls `platform.shutdown()` on each enabled platform to gracefully terminate all active chat sessions.

### Agent Runtime (`src/core/agent/` + `src/core/runtime/`)

- **`core/agent/session.rs`**: Provider-neutral `AgentRuntime` enum; dispatches spawn/send/stop/resume to the active backend.
- **`core/runtime/session.rs`** / **`core/runtime/protocol.rs`**: Claude Code **stream-json** over stdio.
- **`core/agent/cursor_acp.rs`**, **`core/agent/opencode_acp.rs`**: **ACP** JSON-RPC clients (gateway is the ACP *client*; the CLI is the *agent*). Shared helpers: `acp_client.rs`.
- **`core/agent/pi_rpc.rs`**: Pi **line-delimited JSON-RPC** (`pi --mode rpc`).
- **`core/runtime/controller.rs`**: Owns the active `AgentRuntime`, exposes start/stop/send. Emits `ControllerEvent` (Text, Thinking, ToolUse, ToolResult, PermissionRequest, Error, Done). Manages `work_dir`, MCP attach context, and pending permission/resume state.

### Command Routing (`src/core/command/` + `src/core/session/`)

All inbound chat (Feishu / Telegram / QQ / WebUI message API) shares one pipeline:

1. **`CommandRouter::route`** — parse text → `CommandAction` (gateway controls vs forward-to-agent).
2. **`ChatCommandExecutor::execute`** — run side effects (session start/stop, `/cd`, `/models`, permissions, forward).
3. **Presentation** — map `ChatCommandOutcome` to the channel:
   - Bots: cards / keyboards / text + `send_and_poll` + `EventPollSink`
   - WebUI: `session/chat_flow::route_and_execute` → `api/web/handlers/webui_outcome::deliver_chat_outcome` (JSON + SSE)
   - WebUI sidebar: `/api/cmd/*` JSON helpers (`ll`, `pwd`, `cd`) — UI-only, not the chat message path

Entry points:

- **`core/session/chat_flow.rs`**: `route_and_execute(router, executor, context, message)` — call from every platform + WebUI `handle_send_message`.
- **`core/session/channel_command.rs`**: `ChatCommandContext`, `ChatCommandExecutor`, `ChatCommandOutcome`; WebUI uses `with_webui_session` + `SessionStopKind::Webui` for per-tab `/quit`.
- **`core/session/outcome_text.rs`**: plain-text formatting when a bot-style outcome is shown in WebUI chat.
- **`core/command/router.rs`**: Parsing (`route`) and legacy `execute` for `/api/cmd/*` plain-text helpers; no-session forward uses `forward.*` i18n keys in `router.execute`.
- **`core/command/builtin.rs`**: Gateway command implementations used by the executor.

### Platform Layer (`src/platform/`)

See **[Adding a New Chat Platform (Bot)](#adding-a-new-chat-platform-bot)** for the full integration checklist. User-facing setup guides: `docs/bots/` (Feishu, Telegram, QQ — EN + zh-CN).

- **`platform.rs`**: Defines the `Platform` trait (`run()` and `shutdown()`). All platform integrations implement this trait so `DaemonEngine` is platform-agnostic.
- **`platform/feishu.rs`**: WebSocket client for Feishu's pbbp2 protocol (protobuf frames). Gets tenant access token, connects to WS endpoint, handles heartbeats, deduplicates messages, normalizes events into `NormalizedMessage`, routes through `CommandRouter`, and polls `AgentController` events to reply back. Each chat gets its own `ChatSession` (isolated Claude subprocess).
  - `/ll` in Feishu is intercepted before routing: sends an interactive card listing folders from `default_dir`. Card buttons carry `value: { "cmd": "cd", "path": "...", "chat_id": "..." }`.
  - `/cd` in Feishu is intercepted before routing: resolves and canonicalizes the path, enforces that the result stays within `default_dir`, then calls `set_work_dir`.
  - Card callbacks with `cmd == "cd"` are handled directly: call `controller.init_work_dir(path)` and reply with confirmation text.
  - Unknown slash commands when no session is active receive a list of available commands (see `feishu.unknown_command`).
- **`platform/telegram.rs`**: Telegram Bot API integration using long-polling `getUpdates`. Each chat gets its own `TgChatSession` (isolated Claude subprocess). Routes messages through `CommandRouter` and streams Claude responses back via `sendMessage`.
- **`platform/qq.rs`**: QQ 开放平台官方机器人（OpenAPI v2 + Gateway WebSocket）。`app_id` / `app_secret` 换取 access token，连接 `wss` Gateway，处理 `C2C_MESSAGE_CREATE` 与 `GROUP_AT_MESSAGE_CREATE`。频道 id：`u:{openid}`（私聊）、`g:{group_openid}`（群 @）。`/ll` 等为纯文本列表（无卡片按钮）。**MCP `send_file`**: `McpDeliveryTarget::Qq` + 富媒体上传/发送（`platform/qq/api.rs`）；群聊仅支持图片/视频/语音，通用文件需私聊。入站仍为纯文本（无 `inbound_media`）。生产 API：`https://api.sgroup.qq.com`；`qq.sandbox: true` 使用沙箱域名。
- **`platform/proto.rs`**: Protobuf frame codec for Feishu pbbp2 (METHOD_CONTROL / METHOD_DATA, SERVICE_IM / SERVICE_CARD).

### Platform Reference Docs

Official vendor API / console links for building or debugging `src/platform/<name>/` (Feishu, Telegram, QQ), plus the **MCP `send_file` by platform** matrix → **[docs/platform-reference.md](docs/platform-reference.md)**. **When adding a chat platform you must update that file** (and mirror the links under **References** in `docs/bots/<platform>.md` + `.zh-CN.md`). End-user setup is `docs/bots/<platform>.md`, not this reference.

### Configuration (`src/core/config/`)

- **`core/config/loader.rs`**: Loads `~/.cc-gateway/config.json` with `${VAR}` environment variable substitution.
- **`core/config/model.rs`**: `GatewayConfig` with `log`, `agent` (provider profiles), `feishu`, `telegram`, `qq`, plus top-level fields like `port`, `default_dir`, `show_thinking`, `media_retention_days`.
- **`core/config/wizard.rs`**: Interactive setup via `cc-gateway init` (also editable in WebUI Settings).

### Web Server (`src/api/web/`)

- **`api/web/server.rs`**: Axum HTTP server bound to `config.port` (replaces the old throwaway TCP singleton listener). Serves the embedded WebUI static files and exposes REST APIs.
- **`api/web/handlers/ui.rs`**: Static file handler using `rust-embed` to serve the compiled frontend from `webui/dist/` at the root path.
- **`api/web/handlers/session.rs`**: Session APIs — `GET /api/sessions`, `POST /api/sessions`, `DELETE /api/sessions/:id`, `POST /api/sessions/:id/messages`, `GET /api/sessions/:id/history`, `GET /api/sessions/:id/events` (SSE). WebUI **first start** sends `{ provider }` on `POST …/start` (agent picker); **resume after stop** omits `provider` and uses the stored session record.
- **`api/web/handlers/cmd.rs`**: Gateway command APIs — `POST /api/cmd/ll`, `/api/cmd/pwd`, `/api/cmd/cd`, `/api/cmd/cd_default`.
- **`api/web/handlers/config.rs`**: Config APIs — `GET /api/config` (includes `agents` catalog), `POST /api/config`, `GET /api/agents` (integrated provider list + per-profile settings), `GET /api/platforms`.
- **`api/web/handlers/system.rs`**: System APIs — `GET /api/version`, `POST /api/restart`.
- **`api/web/middleware.rs`**: Optional IP allowlist (`allowed_ips`) and WebUI token auth (`webui_token` on `/api` when set in config).

### Session Management (`src/core/session/`)

- **`core/session/channel_manager.rs`**: `ChannelManager` (`GLOBAL_CHANNEL_SESSIONS`) holds channels, agent sessions, and active runtimes (controller + router) in memory.
- **`core/session/channel_model.rs`**: `ChannelSession`, `AgentSession`, `SessionSource` (`WebUI` / `Feishu` / `Telegram` / `QQ`).
- **Persistence**: Channels and agent sessions persist to SQLite (`src/database.rs`, crate alias `db`). On daemon restart, previously active agent subprocesses are gone; records remain for resume/history.

### History Recording

- Events are written to `~/.cc-gateway/history/{session_id}.jsonl`.
- A broadcast channel (`EVENT_BUS`) fans out `ControllerEvent` to both the SSE stream and the history recorder.
- Each line: `{"timestamp": "...", "role": "user|assistant|system", "content": "...", "event_type": "..."}`.

### Database (`src/database.rs`)

- SQLite backend storing channels, agent sessions, config overrides, and runtime state. Auto-creates tables on first access.

### Update / Version Check (`src/update.rs`)

- Checks GitHub Releases (`caixy-plus/cc-gateway`) for newer versions. Used by the WebUI version badge and can be triggered from the sidebar.

### Provider Session ID & Resume

- Claude Code: after spawning the `claude` subprocess, cc-gateway reads `~/.claude/sessions/{pid}.json` to extract Claude's internal session id, persists it as `provider_session_id`, and uses it to resume when supported.
- Cursor / OpenCode ACP: `session/load` with persisted `provider_session_id`, fall back to `session/new` when missing.
- Pi: persists `sessionFile` from RPC `get_state` for gateway records; **provider session resume is not supported** (no `switch_session` on restart — users get a fresh Pi process and `builtin.session_restarted_pi_hint`). `/clear` uses `new_session` and updates the stored file.

## Adding a New Agent Provider

Full checklist for wiring a new CLI/agent — integration styles (stream-json / ACP / custom RPC), backend steps **A–U**, agent registry & WebUI, config shape, verification, naming — lives in **[docs/adding-agent-provider.md](docs/adding-agent-provider.md)**. Current providers: **Claude** (stream-json), **Cursor** & **OpenCode** (ACP), **Pi** (RPC). When adding one, also complete § [User-facing documentation](#user-facing-documentation-keep-in-sync) (agent provider table).

## Adding a New Chat Platform (Bot)

Full checklist for integrating a new chat bot — architecture, transport choice, backend steps **A–U**, platform-specific hooks, frontend (`../cc-gateway-webui`), config shape, init wizard, verification, naming — lives in **[docs/adding-chat-platform.md](docs/adding-chat-platform.md)**. Companion: [docs/platform-integration-checklist.md](docs/platform-integration-checklist.md) (feature-parity matrix + A–E checklist) and § [Platform Reference Docs](#platform-reference-docs). Current platforms: **Feishu** (pbbp2 WebSocket + cards), **Telegram** (Bot API long-polling), **QQ** (OpenAPI v2 Gateway WebSocket). There is **no** `platform_registry` yet — grep existing `feishu`/`telegram`/`qq` match arms when adding one. Also complete § [User-facing documentation](#user-facing-documentation-keep-in-sync).

## Key Patterns

- **Session switching**: `/agent` starts a session per chat (WebUI or bot). Everything except gateway controls is forwarded to the active agent. `/quit` stops the session.
- **Stream-json protocol**: All Claude communication is newline-delimited JSON. Each line is one event. Claude must be launched with `--input-format stream-json --output-format stream-json`.
- **Event channels**: `AgentController` uses an `mpsc::unbounded_channel` to decouple the stdout reader from the consumer (WebUI SSE or platform pollers). Consumers poll `recv_event()`.
- **Detached daemon**: The daemon is a separate OS process. `start()` spawns `cc-gateway _daemon` with stdin/stdout/stderr nulled and a new process group (Unix).
- **Config dir**: `~/.cc-gateway/` holds `config.json`, `daemon.pid`, `logs/`, and `skills/`.

## Internationalization (i18n)

Product UI and bot messages (Feishu, Telegram, QQ, WebUI) are localized via `dict.rs`. For **assistant ↔ user** reply language when editing this repo, see **Response language** under [Development Principles](#development-principles).

All user-facing strings must go through the translation macros in `src/utils/i18n/dict.rs`:

- **Static text**: `crate::t!("module.key")` returns `&str`
- **Formatted text**: `crate::t_fmt!("module.key", NAME = value, ID = id)` returns `String`

### Rules
1. Never hard-code Chinese or English user-visible messages — always add a translation key.
2. Key naming: `{module}.{descriptor}` (e.g. `feishu.permission_title`, `builtin.session_started`, `telegram.shutdown_notice`).
3. Platform-specific keys use the platform prefix (`feishu.`, `telegram.`, `webui.`, `qq.`).
4. Shared / builtin keys use the `builtin.` prefix (`builtin.help`, `builtin.session_stopped`, `builtin.dir_changed`).
5. When adding a new key, provide both English and Chinese (`Language::En` / `Language::ZhCN`) entries in `dict.rs`.
6. Internal debug / tracing messages do not need translation.
