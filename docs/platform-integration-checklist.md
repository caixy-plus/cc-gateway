# Chat Platform Integration Checklist

Use this checklist when adding or substantially changing a **chat bot platform** in cc-gateway. Copy it into your PR description and check off every item. **Do not merge** with unchecked required rows.

English | [简体中文](platform-integration-checklist.zh-CN.md)

## Feature parity reference (current platforms)

| Capability | Feishu | Telegram | QQ |
|------------|--------|----------|-----|
| `Platform` trait (`run` / `shutdown`) | Yes | Yes | Yes |
| Config + `runtime_defaults()` | Yes | Yes | Yes |
| Daemon spawn via `platform_registry` | Yes | Yes | Yes |
| `SessionSource` + DB `source` string | Yes | Yes | Yes |
| Pairing (`require_pairing`) | Yes | Yes | Yes |
| `ChatCommandExecutor` + `CommandRouter` | Yes | Yes | Yes |
| `EventPollSink` (stream replies) | Yes | Yes | Yes |
| **MCP `send_file`** | Yes | Yes | Yes (C2C rich media only; group chat unsupported) |
| `McpContext` on channel commands | Yes | Yes | Yes |
| Deliver-bus text (`spawn_deliver_listener`) | Yes | Yes | Yes |
| WebUI config + `/api/platforms` | Yes | Yes | Yes |
| Init wizard bot step | Yes | Yes | Yes |
| i18n (`<platform>.*`) | Yes | Yes | Yes |
| Inbound media → agent path | Yes | Yes | **Yes** (C2C attachments) |
| Interactive `/ll` / `/agents` UI | Cards | Inline keyboard | Text list |
| Permission prompts UI | Cards / callback | Inline buttons | Text + request id |
| Unknown slash (no session) | Custom help | Help text | Router default |

Document any intentional **No** in your platform’s `docs/bots/<id>.md`.

---

## A. Backend code (required)

| # | Item | Files / notes |
|---|------|----------------|
| A1 | Config struct on `GatewayConfig` | `src/core/config/model.rs` |
| A2 | `Default` + `runtime_defaults()` disable until init | `model.rs` |
| A3 | Restart / live field paths | `src/core/config/platform_registry.rs` + `restart_policy.rs` |
| A4 | Platform module `src/platform/<name>/` | `impl Platform` |
| A5 | `pub mod <name>` | `src/platform.rs` |
| A6 | `PlatformDef` in registry + daemon startup | `src/core/config/platform_registry.rs`, `src/daemon/engine.rs` |
| A7 | Connection status | `src/platform/status.rs` |
| A8 | `SessionSource` variant | `src/core/session/channel_model.rs`, `src/database.rs`, `channel_manager.rs` |
| A9 | `ChatCommandContext::with_mcp_context` | Platform inbound handler |
| A10 | `McpDeliveryTarget::<Platform>` + `FileDelivery` | `src/core/runtime/file_delivery.rs` |
| A11 | `EventPollSink` (incl. permission / confirm / question) | Platform root `platform/<name>.rs` |
| A12 | Deliver listener (if text push needed) | `platform.rs` → `spawn_deliver_listener` |
| A13 | Web config save / mask secrets / platforms API | `src/api/web/handlers/config.rs` |
| A14 | Init wizard menu entry | `src/core/config/wizard.rs` |
| A15 | i18n keys (EN + ZhCN) | `src/utils/i18n/dict.rs` |
| A16 | Tests | Unit tests in the same `.rs`; integration/smoke in `src/tests/` + `tests.rs` when needed |

## B. Platform Reference Docs (required)

Add a **`## <Platform>`** subsection under [docs/platform-reference.md](platform-reference.md) with:

- Developer console URL  
- Auth / token docs  
- Transport actually used (WS / polling / webhook)  
- Inbound event names  
- Outbound send APIs  
- Rich media / file APIs (for MCP)  
- Link to official wiki root  

## C. User-facing documentation (required)

| # | Item |
|---|------|
| C1 | `docs/bots/<platform>.md` + `.zh-CN.md` (setup, pairing, limits, **References**) |
| C2 | `docs/bots/README.md` + `.zh-CN.md` (table row + MCP column) |
| C3 | `docs/config.md` + `.zh-CN.md` (config section + example JSON) |
| C4 | `docs/usage.md` + `.zh-CN.md` (usage section) |
| C5 | `README.md` + `.zh-CN.md` (features, quick-start table) |
| C6 | `scripts/install-docs.sh` + `.ps1` (post-install links) |
| C7 | [CLAUDE.md](../CLAUDE.md) — current platforms list, MCP matrix, architecture bullet |

## D. Frontend (`../cc-gateway-webui`) (required until dynamic platforms API)

| # | Item |
|---|------|
| D1 | `GatewayConfig.<platform>` in `types/index.ts` |
| D2 | `SettingsModal.tsx` section |
| D3 | `en.ts` / `zh.ts` settings strings |
| D4 | `SessionList` / `PairingModal` platform labels (if needed) |

## E. Verification (required)

1. `cargo test` for new/changed modules + `config_model` + `restart_policy`  
2. `cc-gateway init` — enable platform, valid credentials warning if empty  
3. `cc-gateway start` — platform shows connected in logs / WebUI  
4. Pairing flow when `require_pairing: true`  
5. `/agent`, message round-trip, `/cd`, `/ll`, `/quit`  
6. MCP `send_file` with Claude (or other MCP-capable provider) — if supported  
7. Daemon restart after credential change; live toggle for `require_pairing`  
8. Install script prints new doc URLs  

---

## Agent provider checklist (short)

For **agents** (not chat platforms), see [docs/adding-agent-provider.md](adding-agent-provider.md) and update:

- `agent_registry.rs`, `config/model.rs`, `docs/config.md`, README provider list  
- No `docs/bots/` unless platform-specific  
