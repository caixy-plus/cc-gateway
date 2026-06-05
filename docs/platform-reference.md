# Platform Reference Docs

> Back: [CLAUDE.md](../CLAUDE.md) · [README](../README.md) · User setup guides: [docs/bots](bots/README.md)

**For implementers** — official vendor API / console links used when building or debugging `src/platform/<name>/`. This section is **not** end-user setup; users read `docs/bots/<platform>.md`.

**When adding a new chat platform, you must update this section** (and mirror the same links under **References** in `docs/bots/<platform>.md` + `.zh-CN.md`):

1. **Console / developer portal** — where to create the app and copy credentials.
2. **Auth** — token, app secret, or bot token docs.
3. **Transport** — the API surface cc-gateway actually uses (WebSocket opcodes, `getUpdates`, webhook, etc.).
4. **Inbound events** — message / callback event names and payloads.
5. **Outbound messages** — send message / card / keyboard APIs.
6. **Optional** — intents, permissions, rate limits, sandbox vs production hosts.

Also refresh [Adding a New Chat Platform](adding-chat-platform.md) (“Current platforms” line) and the transport table in step **2** if the style is new.

## Feishu / Lark (`platform/feishu/`)

| Topic | URL |
|-------|-----|
| Open Platform (home) | https://open.feishu.cn/document/home/index |
| Create app (CN console) | https://open.feishu.cn/app |
| Lark (intl console) | https://open.larksuite.com/app |
| Bot long connection / WebSocket | https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/server-side-sdk/golang-sdk-guide/preparations |
| Event `im.message.receive_v1` | Search in Feishu docs for “接收消息” / message events |
| Card JSON v2 overview | https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes |
| Button component (V2) | https://open.feishu.cn/document/feishu-cards/card-json-v2-components/interactive-components/button |

**cc-gateway:** pbbp2 WebSocket (`platform/feishu/ws.rs`, `platform/proto/`); interactive cards (`platform/feishu/cards.rs`). User guide: [bots/feishu.md](bots/feishu.md).

## Telegram (`platform/telegram/`)

| Topic | URL |
|-------|-----|
| Bot API reference | https://core.telegram.org/bots/api |
| Long polling `getUpdates` | https://core.telegram.org/bots/api#getupdates |
| `sendMessage` | https://core.telegram.org/bots/api#sendmessage |
| Create bot (BotFather) | https://t.me/BotFather |

**cc-gateway:** HTTP long-polling only (no webhook in tree). User guide: [bots/telegram.md](bots/telegram.md).

## QQ (`platform/qq/`)

| Topic | URL |
|-------|-----|
| Developer portal (console) | https://q.qq.com/ |
| API v2 wiki (home) | https://bot.q.qq.com/wiki/develop/api-v2/ |
| WebSocket gateway (opcodes, Identify/Resume) | https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/reference.html |
| Events / intents | https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/event-emit.html |
| Get WSS URL (`GET /gateway` / `gateway/bot`) | https://bot.q.qq.com/wiki/develop/api-v2/openapi/wss/url_get.html |
| Messages (send/receive) | https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/send-receive/ |
| Rich media (`send_file` / inbound) | https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/send-receive/rich-media.html |
| Access token (`getAppAccessToken`) | Implemented against `https://bots.qq.com/app/getAppAccessToken` (see `platform/qq/api.rs`) |

**cc-gateway:** OpenAPI v2 WebSocket Gateway (`platform/qq/ws.rs`); **C2C chat only** (group @ → `qq.group_chat_unsupported`); C2C inbound attachments via `extract_inbound_attachments`. Production API `https://api.sgroup.qq.com`, sandbox `https://sandbox.api.sandbox.qq.com`. User guide: [bots/qq.md](bots/qq.md).

## MCP `send_file` by platform

Agents can call the gateway MCP tool **`send_file`** (see `core/runtime/mcp_server.rs`) to push a local file into the **active chat**. This requires:

1. `McpDeliveryTarget::<Platform>(…)` in `core/runtime/file_delivery.rs` with a `FileDelivery` impl.
2. Platform builds `McpContext { delivery: … }` and passes `ChatCommandContext::with_mcp_context(...)` when starting a session (Feishu/Telegram pattern).
3. Provider supports MCP attach (`core/agent/mcp_attach.rs` — Claude + ACP providers).

| Platform | MCP `send_file` | Notes |
|----------|-----------------|-------|
| **Feishu** | Yes | Images: `im/v1/images` (`image_type=message`) + `msg_type=image` with `image_key` (inline preview). Other files: `im/v1/files` + `msg_type=file`. Image upload max **10 MB** (Feishu API). |
| **Telegram** | Yes | Images: [`sendPhoto`](https://core.telegram.org/bots/api#sendphoto) multipart `photo`. Other files: `sendDocument`. |
| **QQ** | **Yes** (C2C only) | Rich media upload + `msg_type` 7. **Inline images:** `file_type=1`, **PNG/JPG only**. C2C: images, video, voice, generic files. **Group chat not supported** for messaging. |
| **WebUI** | Yes | `McpDeliveryTarget::WebUi` — files land in chat via `GET /api/media/{id}`; user upload via `POST /api/sessions/{id}/upload` |
| **CLI only** | N/A | No chat delivery target |

When adding a chat platform, **document** MCP support in this table, `docs/bots/<platform>.md`, and the platform hooks table below. If not implemented on day one, state it explicitly so users do not expect agent file push.

## User-facing setup guides

End-user / operator walkthroughs: **`docs/bots/`** (EN + `*.zh-CN.md` per platform), index [bots/README.md](bots/README.md). Install scripts list these via `scripts/install-docs.*`.
