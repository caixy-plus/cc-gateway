# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cc-gateway is a Rust gateway that exposes local agent sessions to remote users via chat bot platforms (Feishu/Lark, Telegram, QQ) and WebUI. It spawns provider CLIs (e.g. `claude`, `codex-acp`, Cursor `agent acp`, `opencode acp`, `kimi acp`, `gemini --acp`), communicates over stdin/stdout, and bridges messages between the provider and external interfaces.

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
└── tests/               # cross-module integration / smoke tests only (`#[cfg(test)]` in lib)
```

**Tests:** put **unit tests** in the same `.rs` file (`#[cfg(test)] mod tests` at the bottom). Use `src/tests/` only for flows that need fake CLIs, DB, HTTP, or global session state; register new modules in `src/tests.rs`. Do not widen `pub`/`pub(crate)` on production helpers just to test from another file.

- Embeds WebUI static files via `rust-embed` (`src/api/web/handlers/ui.rs` → `webui/dist/`).
- Internal code may use `crate::config::…` via `lib.rs` re-exports; prefer `crate::core::config::…` in new code.
- **Frontend** (separate repo): React 18 + Vite + TypeScript. Lives at `../cc-gateway-webui` (or clone from `https://github.com/caixy-plus/cc-gateway-webui.git`).

Workflow: edit frontend → `npm run build` in `cc-gateway-webui` → copy `dist/` into this repo's `webui/dist/` → rebuild Rust binary.

Notes:
- The frontend repo is a **sibling directory** (`../cc-gateway-webui`); clone it if missing. If `webui/dist/` is absent/unembedded, the WebUI serves a fallback page.
- **NEVER commit `webui/dist/`** — gitignored, a **local build artifact** only; never `git add -f` it. Stage only `src/`, `Cargo.toml`, etc.
- **Integration is automatic — don't hand-run `npm run build` + copy `dist/`.** Local: `./install_local.sh` builds the frontend and `cargo build --release` embeds it. Release: CI builds the **frontend repo's GitHub `main`** into `webui/dist/`. So after editing the frontend, commit/push the **frontend repo**, then `./install_local.sh` (local) or push a tag (release). Release-tag ordering & rationale → [docs/release.md](docs/release.md) / [release.zh-CN.md](docs/release.zh-CN.md).

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
- **Git branch naming**: feature work uses `feature/<kebab-slug>` (e.g. `feature/platform-registry-webui-files-models`). Do **not** use `feat/` or untyped branch names unless the user says otherwise. Apply the same branch name in **cc-gateway** and **cc-gateway-webui** when both repos change. See `.cursor/rules/git-branch-naming.mdc`.
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

Treat documentation as part of the feature: adding or materially changing an **agent provider** or **chat platform** is not complete until the per-file sync tables are satisfied (EN + zh-CN where paired). **Full checklist → [docs/doc-sync-checklist.md](docs/doc-sync-checklist.md)** (new-provider table, new-platform table, bilingual/single-source/install-output conventions).

## Architecture

### Entry Points

- **`src/main.rs`**: Binary entry. No subcommand → prints help (same as `--help`, exit 0). Subcommands: `start`/`stop`/`restart`/`log`/`status`/`enable`/`disable`/`init`/`webui`/…; hidden `_daemon` runs `DaemonEngine`.

### Daemon Lifecycle (`src/daemon/`)

- **`daemon.rs`**: PID-file-based daemon management with triple singleton lock: port binding (configurable via `port`), `.daemon-starting.lock` for `start()` atomicity, and PID file `flock` held for daemon lifetime. `start()` spawns a detached child running `cc-gateway _daemon`. `stop()` sends SIGTERM (Unix) or `taskkill` (Windows). `run()` loads config, writes PID file, and starts `DaemonEngine`.
- **`daemon/engine.rs`**: Core async engine. Starts all enabled `Platform` integrations (`feishu.enabled`, `telegram.enabled`, `qq.enabled`) concurrently, then waits for shutdown signal (SIGTERM/SIGINT). On shutdown, calls `platform.shutdown()` on each enabled platform to gracefully terminate all active chat sessions.

### Agent Runtime (`src/core/agent/` + `src/core/runtime/`)

- **`core/agent/session.rs`**: Provider-neutral `AgentRuntime` enum; spawn/stop still match here; send/compact/models dispatch via [`core/agent/backend.rs`](src/core/agent/backend.rs) [`AgentBackend`](src/core/agent/backend.rs) trait + `dispatch_agent_backend!` macro.
- **`core/agent/backend.rs`**: Unified gateway-facing session API (`send_user_message`, `set_model`, `compact_context`, …). Claude stream-json, Pi RPC, and ACP sessions each implement `AgentBackend`.
- **`core/runtime/session.rs`** / **`core/runtime/protocol.rs`**: Claude Code **stream-json** over stdio (`StreamJsonSession` implements `AgentBackend`).
- **`core/agent/acp_session.rs`**: Shared **ACP** spawn/prompt/permission/update logic (`GenericAcpSession<H: AcpHooks>`). Provider-specific argv, MCP prep, auth, and extension notifications live in thin `AcpHooks` impls.
- **`core/agent/codex_acp.rs`**, **`core/agent/cursor_acp.rs`**, **`core/agent/opencode_acp.rs`**, **`core/agent/kimi_acp.rs`**, **`core/agent/gemini_acp.rs`**: thin `AcpHooks` impls only — transport via `acp_client.rs`. Codex and Gemini skip the `authenticate` RPC (cached CLI credentials); Codex applies `mode` post-spawn via `session/set_mode`.
- **`core/agent/pi_rpc.rs`**: Pi **line-delimited JSON-RPC** (`pi --mode rpc`).
- **`core/config/agent_registry.rs`**: `AgentCapabilities` + `AGENT_PROVIDER_DEFS` — single source for `/models`, MCP attach, `/compact`, resume, and `default_args` normalization flags.
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
- **`platform/feishu.rs`**: WebSocket client for Feishu's pbbp2 protocol (protobuf frames). Inbound text uses shared `chat_flow::route_and_execute` → `ChatCommandOutcome` (cards for `/ll`, `ListDir`, permissions, etc.). Card callbacks with `cmd == "cd"` are handled in `feishu/ws.rs`. Each chat gets its own isolated agent subprocess.
- **`platform/telegram.rs`**: Telegram Bot API integration using long-polling `getUpdates`. Shared command pipeline; `/ll` and `/agents` render as **inline keyboards**. Streams agent events back via `sendMessage`.
- **`platform/qq.rs`**: QQ 开放平台官方机器人（OpenAPI v2 + Gateway WebSocket）。**仅 C2C 私聊**；`GROUP_AT_MESSAGE_CREATE` 回复 `qq.group_chat_unsupported`。频道 id：`u:{openid}`。`/ll` 等为纯文本。**入站附件**（C2C）经 `inbound_media` 转发。**MCP `send_file`**: `McpDeliveryTarget::Qq`（C2C 富媒体）。生产 API：`https://api.sgroup.qq.com`；`qq.sandbox: true` 使用沙箱域名。
- **`platform/proto.rs`**: Protobuf frame codec for Feishu pbbp2 (METHOD_CONTROL / METHOD_DATA, SERVICE_IM / SERVICE_CARD).

### Platform Reference Docs

Official vendor API / console links for building or debugging `src/platform/<name>/` (Feishu, Telegram, QQ), plus the **MCP `send_file` by platform** matrix → **[docs/platform-reference.md](docs/platform-reference.md)**. **When adding a chat platform you must update that file** (and mirror the links under **References** in `docs/bots/<platform>.md` + `.zh-CN.md`). End-user setup is `docs/bots/<platform>.md`, not this reference.

### Configuration (`src/core/config/`)

- **`core/config/loader.rs`**: Loads `~/.cc-gateway/config.json` with `${VAR}` substitution; `upgrade_config_json` (flat `agent.<id>` → `agent.providers.<id>`, top-level platforms) + `validate_agent_profile_keys` + `normalize_profiles`; **persists** the file when legacy layout or missing registry provider entries were upgraded.
- **`core/config/model.rs`**: `GatewayConfig` with `log`, `agent` (`default` + `providers` map), `platforms` (`feishu` / `telegram` / `qq` sections), plus top-level fields like `port`, `default_dir`, `show_thinking`, `media_retention_days`.
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
- Codex / Cursor / OpenCode / Kimi / Gemini ACP: `session/load` with persisted `provider_session_id`, fall back to `session/new` when missing.
- Pi: persists `sessionFile` from RPC `get_state` for gateway records; **provider session resume is not supported** (no `switch_session` on restart — users get a fresh Pi process and `builtin.session_restarted_pi_hint`). `/clear` uses `new_session` and updates the stored file.

## Adding a New Agent Provider

Full checklist for wiring a new CLI/agent — integration styles (stream-json / ACP / custom RPC), backend steps **A–U**, agent registry & WebUI, config shape, verification, naming — lives in **[docs/adding-agent-provider.md](docs/adding-agent-provider.md)**. Current providers: **Claude** (stream-json), **Codex** (ACP via `codex-acp`), **Cursor**, **OpenCode**, **Kimi** & **Gemini** (ACP), **Pi** (RPC). When adding one, also complete § [User-facing documentation](#user-facing-documentation-keep-in-sync) (agent provider table).

## Adding a New Chat Platform (Bot)

Full checklist for integrating a new chat bot — architecture, transport choice, backend steps **A–U**, platform-specific hooks, frontend (`../cc-gateway-webui`), config shape, init wizard, verification, naming — lives in **[docs/adding-chat-platform.md](docs/adding-chat-platform.md)**. Companion: [docs/platform-integration-checklist.md](docs/platform-integration-checklist.md) (feature-parity matrix + A–E checklist) and § [Platform Reference Docs](#platform-reference-docs). Current platforms: **Feishu**, **Telegram**, **QQ**. Phase 1 **`platform_registry`** (`src/core/config/platform_registry.rs`) centralizes daemon spawn, status, APIs, pairing, and restart policy — still add typed config + `src/platform/<name>/` per checklist. Also complete § [User-facing documentation](#user-facing-documentation-keep-in-sync).

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
