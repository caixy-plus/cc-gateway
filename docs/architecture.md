> Back: [CLAUDE.md](../CLAUDE.md). Companion: [Adding a New Agent Provider](adding-agent-provider.md), [Adding a New Chat Platform](adding-chat-platform.md), [Platform Reference Docs](platform-reference.md).

# Architecture

Module-by-module map of the cc-gateway backend. CLAUDE.md keeps only the high-level rules; this is the detailed reference, read on demand.

## Source layout

```
src/
├── main.rs              # CLI entry → cc_gateway::
├── lib.rs               # layers + crate-root re-exports (config, web, db, …)
├── core/                # agent, command, config, history, prompt, runtime, session
├── api/web/             # Axum HTTP + WebUI handlers
├── database.rs          # SQLite (crate alias `db`)
├── platform/            # Feishu, Telegram
├── daemon/              # PID, engine, lifecycle
├── utils/               # env helpers + i18n
├── types.rs             # shared type re-exports
├── update.rs, uninstall.rs
└── tests/               # cross-module integration / smoke tests only (`#[cfg(test)]` in lib)
```

- Embeds WebUI static files via `rust-embed` (`src/api/web/handlers/ui.rs` → `webui/dist/`).
- Internal code may use `crate::config::…` via `lib.rs` re-exports; prefer `crate::core::config::…` in new code.

## Entry Points

- **`src/main.rs`**: Binary entry. No subcommand → prints help (same as `--help`, exit 0). Subcommands: `start`/`stop`/`restart`/`log`/`status`/`enable`/`disable`/`init`/`webui`/…; hidden `_daemon` runs `DaemonEngine`.

## Daemon Lifecycle (`src/daemon/`)

- **`daemon.rs`**: PID-file-based daemon management with triple singleton lock: port binding (configurable via `port`), `.daemon-starting.lock` for `start()` atomicity, and PID file `flock` held for daemon lifetime. `start()` spawns a detached child running `cc-gateway _daemon`. `stop()` sends SIGTERM (Unix) or `taskkill` (Windows). `run()` loads config, writes PID file, and starts `DaemonEngine`.
- **`daemon/engine.rs`**: Core async engine. Starts all enabled `Platform` integrations (`feishu.enabled`, `telegram.enabled`) concurrently, then waits for shutdown signal (SIGTERM/SIGINT). On shutdown, calls `platform.shutdown()` on each enabled platform to gracefully terminate all active chat sessions.

## Agent Runtime (`src/core/agent/` + `src/core/runtime/`)

- **`core/agent/session.rs`**: Provider-neutral `AgentRuntime` enum; spawn/stop still match here; send/compact/models dispatch via [`core/agent/backend.rs`](../src/core/agent/backend.rs) [`AgentBackend`](../src/core/agent/backend.rs) trait + `dispatch_agent_backend!` macro.
- **`core/agent/backend.rs`**: Unified gateway-facing session API (`send_user_message`, `set_model`, `compact_context`, …). Claude stream-json, Pi RPC, and ACP sessions each implement `AgentBackend`.
- **`core/runtime/session.rs`** / **`core/runtime/protocol.rs`**: Claude Code **stream-json** over stdio (`StreamJsonSession` implements `AgentBackend`).
- **`core/agent/acp_session.rs`**: Shared **ACP** spawn/prompt/permission/update logic (`GenericAcpSession<H: AcpHooks>`). Provider-specific argv, MCP prep, auth, and extension notifications live in thin `AcpHooks` impls.
- **`core/agent/codex_acp.rs`**, **`core/agent/cursor_acp.rs`**, **`core/agent/opencode_acp.rs`**, **`core/agent/kimi_acp.rs`**, **`core/agent/gemini_acp.rs`**, **`core/agent/qoder_acp.rs`**: thin `AcpHooks` impls only — transport via `acp_client.rs`. Codex, Gemini, and Qoder skip the `authenticate` RPC (cached CLI credentials); Codex applies `mode` post-spawn via `session/set_mode`.
- **`core/agent/pi_rpc.rs`**: Pi **line-delimited JSON-RPC** (`pi --mode rpc`).
- **`core/config/agent_registry.rs`**: `AgentCapabilities` + `AGENT_PROVIDER_DEFS` — single source for `/models`, MCP attach, `/compact`, resume, and `default_args` normalization flags.
- **`core/runtime/controller.rs`**: Owns the active `AgentRuntime`, exposes start/stop/send. Emits `ControllerEvent` (Text, Thinking, ToolUse, ToolResult, PermissionRequest, Error, Done). Manages `work_dir`, MCP attach context, and pending permission/resume state.

## Command Routing (`src/core/command/` + `src/core/session/`)

All inbound chat (Feishu / Telegram / WebUI message API) shares one pipeline:

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

## Platform Layer (`src/platform/`)

See **[Adding a New Chat Platform (Bot)](../CLAUDE.md#adding-a-new-chat-platform-bot)** for the full integration checklist. User-facing setup guides: `docs/bots/` (Feishu, Telegram — EN + zh-CN).

- **`platform.rs`**: Defines the `Platform` trait (`run()` and `shutdown()`). All platform integrations implement this trait so `DaemonEngine` is platform-agnostic.
- **`platform/feishu.rs`**: WebSocket client for Feishu's pbbp2 protocol (protobuf frames). Inbound text uses shared `chat_flow::route_and_execute` → `ChatCommandOutcome` (cards for `/ll`, `ListDir`, permissions, etc.). Card callbacks with `cmd == "cd"` are handled in `feishu/ws.rs`. Each chat gets its own isolated agent subprocess.
- **`platform/telegram.rs`**: Telegram Bot API integration using long-polling `getUpdates`. Shared command pipeline; `/ll` and `/agents` render as **inline keyboards**. Streams agent events back via `sendMessage`.
- **`platform/proto.rs`**: Protobuf frame codec for Feishu pbbp2 (METHOD_CONTROL / METHOD_DATA, SERVICE_IM / SERVICE_CARD).

### Platform Reference Docs

Official vendor API / console links for building or debugging `src/platform/<name>/` (Feishu, Telegram), plus the **MCP `send_file` by platform** matrix → **[platform-reference.md](platform-reference.md)**. **When adding a chat platform you must update that file** (and mirror the links under **References** in `docs/bots/<platform>.md` + `.zh-CN.md`). End-user setup is `docs/bots/<platform>.md`, not this reference.

## Configuration (`src/core/config/`)

- **`core/config/loader.rs`**: Loads `~/.cc-gateway/config.json` with `${VAR}` substitution; `upgrade_config_json` (flat `agent.<id>` → `agent.providers.<id>`, top-level platforms) + `validate_agent_profile_keys` + `normalize_profiles`; **persists** the file when legacy layout or missing registry provider entries were upgraded.
- **`core/config/model.rs`**: `GatewayConfig` with `log`, `agent` (`default` + `providers` map), `platforms` (`feishu` / `telegram` sections), plus top-level fields like `port`, `default_dir`, `show_thinking`, `media_retention_days`.
- **`core/config/wizard.rs`**: Interactive setup via `cc-gateway init` (also editable in WebUI Settings).

## Web Server (`src/api/web/`)

- **`api/web/server.rs`**: Axum HTTP server bound to `config.port` (replaces the old throwaway TCP singleton listener). Serves the embedded WebUI static files and exposes REST APIs.
- **`api/web/handlers/ui.rs`**: Static file handler using `rust-embed` to serve the compiled frontend from `webui/dist/` at the root path.
- **`api/web/handlers/session.rs`**: Session APIs — `GET /api/sessions`, `POST /api/sessions`, `DELETE /api/sessions/:id`, `POST /api/sessions/:id/messages`, `GET /api/sessions/:id/history`, `GET /api/sessions/:id/events` (SSE). WebUI **first start** sends `{ provider }` on `POST …/start` (agent picker); **resume after stop** omits `provider` and uses the stored session record.
- **`api/web/handlers/cmd.rs`**: Gateway command APIs — `POST /api/cmd/ll`, `/api/cmd/pwd`, `/api/cmd/cd`, `/api/cmd/cd_default`.
- **`api/web/handlers/config.rs`**: Config APIs — `GET /api/config` (includes `agents` catalog), `POST /api/config`, `GET /api/agents` (integrated provider list + per-profile settings), `GET /api/platforms`.
- **`api/web/handlers/system.rs`**: System APIs — `GET /api/version`, `POST /api/restart`.
- **`api/web/middleware.rs`**: Optional IP allowlist (`allowed_ips`) and WebUI token auth (`webui_token` on `/api` when set in config).

## Session Management (`src/core/session/`)

- **`core/session/channel_manager.rs`**: `ChannelManager` (`GLOBAL_CHANNEL_SESSIONS`) holds channels, agent sessions, and active runtimes (controller + router) in memory.
- **`core/session/channel_model.rs`**: `ChannelSession`, `AgentSession`, `SessionSource` (`WebUI` / `Feishu` / `Telegram`).
- **Persistence**: Channels and agent sessions persist to SQLite (`src/database.rs`, crate alias `db`). On daemon restart, previously active agent subprocesses are gone; records remain for resume/history.

## History Recording

- Events are written to `~/.cc-gateway/history/{session_id}.jsonl`.
- A broadcast channel (`EVENT_BUS`) fans out `ControllerEvent` to both the SSE stream and the history recorder.
- Each line: `{"timestamp": "...", "role": "user|assistant|system", "content": "...", "event_type": "..."}`.

## Database (`src/database.rs`)

- SQLite backend storing channels, agent sessions, config overrides, and runtime state. Auto-creates tables on first access.

## Update / Version Check (`src/update.rs`)

- Checks GitHub Releases (`caixy-plus/cc-gateway`) for newer versions. Used by the WebUI version badge and can be triggered from the sidebar.

## Provider Session ID & Resume

- Claude Code: after spawning the `claude` subprocess, cc-gateway reads `~/.claude/sessions/{pid}.json` to extract Claude's internal session id, persists it as `provider_session_id`, and uses it to resume when supported. In-session model switch respawns with `--resume <id> --model <model>` (preserves context); an unavailable model that makes Claude exit triggers a rollback to the previous model.
- Codex / Cursor / OpenCode / Kimi / Gemini / Qoder ACP: `session/load` with persisted `provider_session_id`, fall back to `session/new` when missing.
- Pi: persists `sessionFile` from RPC `get_state` for gateway records; **provider session resume is not supported** (no `switch_session` on restart — users get a fresh Pi process and `builtin.session_restarted_pi_hint`). `/clear` uses `new_session` and updates the stored file.
