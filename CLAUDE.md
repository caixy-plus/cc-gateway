# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

cc-gateway is a Rust gateway that exposes local agent sessions to remote users via chat bot platforms (Feishu/Lark, Telegram, QQ) and WebUI. It spawns provider CLIs (e.g. `claude`, `codex-acp`, Cursor `agent acp`, `opencode acp`, `kimi acp`, `gemini --acp`, `qoderclicn --acp`), communicates over stdin/stdout, and bridges messages between the provider and external interfaces.

## Project Structure

This is a **frontend/backend split** project. **Module-by-module map + source layout → [docs/architecture.md](docs/architecture.md).**

- **Backend** (this repo): Rust **library** (`src/lib.rs`, crate `cc_gateway`) + thin binary (`src/main.rs`); layered modules under `src/core`, `src/api/web`, `src/platform`, `src/daemon`, `src/utils`. Internal code may use `crate::config::…` re-exports; prefer `crate::core::config::…` in new code.
- **Tests:** put **unit tests** in the same `.rs` file (`#[cfg(test)] mod tests` at the bottom). Use `src/tests/` only for flows that need fake CLIs, DB, HTTP, or global session state; register new modules in `src/tests.rs`. Do not widen `pub`/`pub(crate)` on production helpers just to test from another file.
- **Frontend** (separate repo): React 18 + Vite + TypeScript. **Sibling directory** at `../cc-gateway-webui` (clone from `https://github.com/caixy-plus/cc-gateway-webui.git` if missing). Embedded via `rust-embed` from `webui/dist/`; if absent, the WebUI serves a fallback page.
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

Full module-by-module reference (entry points, daemon lifecycle, agent runtime, command routing, platform layer, config, web server, session management, history, DB, provider session-id & resume) → **[docs/architecture.md](docs/architecture.md)**.

Quick orientation: all inbound chat (Feishu / Telegram / QQ / WebUI) shares one pipeline — `CommandRouter::route` → `ChatCommandExecutor::execute` → per-channel presentation, entered via `core/session/chat_flow::route_and_execute`. Provider sessions dispatch through the `AgentBackend` trait (`core/agent/backend.rs`): Claude = stream-json, ACP providers via `acp_session.rs`, Pi = JSON-RPC.

## Adding a New Agent Provider

Full checklist for wiring a new CLI/agent — integration styles (stream-json / ACP / custom RPC), backend steps **A–U**, agent registry & WebUI, config shape, verification, naming — lives in **[docs/adding-agent-provider.md](docs/adding-agent-provider.md)**. Current providers: **Claude** (stream-json), **Codex** (ACP via `codex-acp`), **Cursor**, **OpenCode**, **Kimi**, **Gemini** & **Qoder** (ACP), **Pi** (RPC). When adding one, also complete § [User-facing documentation](#user-facing-documentation-keep-in-sync) (agent provider table).

## Adding a New Chat Platform (Bot)

Full checklist for integrating a new chat bot — architecture, transport choice, backend steps **A–U**, platform-specific hooks, frontend (`../cc-gateway-webui`), config shape, init wizard, verification, naming — lives in **[docs/adding-chat-platform.md](docs/adding-chat-platform.md)**. Companion: [docs/platform-integration-checklist.md](docs/platform-integration-checklist.md) (feature-parity matrix + A–E checklist) and [docs/platform-reference.md](docs/platform-reference.md) (vendor API links + MCP `send_file` matrix). Current platforms: **Feishu**, **Telegram**, **QQ**. Phase 1 **`platform_registry`** (`src/core/config/platform_registry.rs`) centralizes daemon spawn, status, APIs, pairing, and restart policy — still add typed config + `src/platform/<name>/` per checklist. Also complete § [User-facing documentation](#user-facing-documentation-keep-in-sync).

## Key Patterns

- **Session switching**: `/agent` starts a session per chat (WebUI or bot). Everything except gateway controls is forwarded to the active agent. `/quit` stops the session.
- **Stream-json protocol**: All Claude communication is newline-delimited JSON. Each line is one event. Claude must be launched with `--input-format stream-json --output-format stream-json`.
- **Event channels**: `AgentController` uses an `mpsc::unbounded_channel` to decouple the stdout reader from the consumer (WebUI SSE or platform pollers). Consumers poll `recv_event()`.
- **Detached daemon**: The daemon is a separate OS process. `start()` spawns `cc-gateway _daemon` with stdin/stdout/stderr nulled and a new process group (Unix).
- **Config dir**: `~/.cc-gateway/` holds `config.json`, `daemon.pid`, `logs/`, and `skills/`.

## Internationalization (i18n)

Never hard-code user-visible strings: route every product UI / bot message through `crate::t!("module.key")` or `crate::t_fmt!("module.key", NAME = v)` in `src/utils/i18n/dict.rs`, and add **both** `Language::En` and `Language::ZhCN` entries. Key naming, prefixes, and full rules → **[docs/i18n.md](docs/i18n.md)**. (Assistant ↔ user reply language is separate — see **Response language** under [Development Principles](#development-principles).)
