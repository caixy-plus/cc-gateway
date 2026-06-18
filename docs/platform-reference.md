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
| Receive events through WebSocket | https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case |
| Event `im.message.receive_v1` | https://open.feishu.cn/document/server-docs/im-v1/message/events/receive |
| Card callback event `card.action.trigger` | https://open.feishu.cn/document/feishu-cards/card-callback-communication |
| Configure card interactions | https://open.feishu.cn/document/feishu-cards/configuring-card-interactions |
| Handle card callbacks | https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/handle-card-callbacks |
| Receive callbacks through WebSocket | https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address |
| Send message | https://open.feishu.cn/document/server-docs/im-v1/message/create |
| Upload image | https://open.feishu.cn/document/server-docs/im-v1/image/create |
| Upload file | https://open.feishu.cn/document/server-docs/im-v1/file/create |
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

## MCP `send_file` by platform

Agents can call the gateway MCP tool **`send_file`** (see `core/runtime/mcp_server.rs`) to push a local file into the **active chat**. This requires:

1. `McpDeliveryTarget::<Platform>(…)` in `core/runtime/file_delivery.rs` with a `FileDelivery` impl.
2. Platform builds `McpContext { delivery: … }` and passes `ChatCommandContext::with_mcp_context(...)` when starting a session (Feishu/Telegram pattern).
3. Provider supports MCP attach (`core/agent/mcp_attach.rs` — Claude + ACP providers).

| Platform | MCP `send_file` | Notes |
|----------|-----------------|-------|
| **Feishu** | Yes | Images: `im/v1/images` (`image_type=message`) + `msg_type=image` with `image_key` (inline preview). Other files: `im/v1/files` + `msg_type=file`. Upload limits: images **10 MB**, files **30 MB** (Feishu API). |
| **Telegram** | Yes | Images: [`sendPhoto`](https://core.telegram.org/bots/api#sendphoto) multipart `photo`. Other files: `sendDocument`. |
| **WebUI** | Yes | `McpDeliveryTarget::WebUi` — files land in chat via `GET /api/media/{id}`; user upload via `POST /api/sessions/{id}/upload` |
| **CLI only** | N/A | No chat delivery target |

When adding a chat platform, **document** MCP support in this table, `docs/bots/<platform>.md`, and the platform hooks table below. If not implemented on day one, state it explicitly so users do not expect agent file push.

## User-facing setup guides

End-user / operator walkthroughs: **`docs/bots/`** (EN + `*.zh-CN.md` per platform), index [bots/README.md](bots/README.md). Install scripts list these via `scripts/install-docs.*`.
