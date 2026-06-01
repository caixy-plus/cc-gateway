# Chat Platform Bot Setup

cc-gateway can bridge local agent sessions to multiple chat bots at once. Each platform runs as an independent integration inside the daemon; enable any combination via `enabled: true` in `~/.cc-gateway/config.json`.

| Platform | Guide | Transport | MCP `send_file` | Config section |
|----------|-------|-------------|-----------------|---------------|
| Feishu / Lark | [feishu.md](feishu.md) | WebSocket (pbbp2) | Yes | `feishu` |
| Telegram | [telegram.md](telegram.md) | HTTP long-polling (`getUpdates`) | Yes | `telegram` |
| QQ (official bot) | [qq.md](qq.md) | WebSocket Gateway (OpenAPI v2) | Yes (group: media only) | `qq` |

中文文档：[README.zh-CN.md](README.zh-CN.md)

## Quick start (any platform)

1. **Install** cc-gateway and run `cc-gateway init` (or edit `~/.cc-gateway/config.json` / WebUI Settings).
2. **Fill credentials** for the bot(s) you want (see per-platform guides).
3. **Start the daemon:** `cc-gateway start` (or `restart` after credential changes).
4. **Open WebUI** at `http://127.0.0.1:<port>/` (default port `17534`) to check platform status and pairing.

## Pairing (recommended for bots exposed to others)

When `require_pairing` is `true` (default for all platforms), a **new** chat must be approved in the WebUI **Pairing** panel before messages are forwarded to an agent.

1. User sends any message to the bot → bot replies with a pairing code.
2. Admin opens WebUI → Pairing → approves the request (or enters the code).
3. User can then use `/agent`, `/help`, and normal chat.

You can turn pairing off per platform in config or WebUI Settings (`require_pairing: false`). Disabling pairing means anyone who can message the bot can use it—only do this for private/test bots.

## Running multiple platforms

The daemon starts **every** platform with `"enabled": true` in parallel. Example:

```json
{
  "feishu": { "enabled": true, "app_id": "...", "app_secret": "...", "require_pairing": true },
  "telegram": { "enabled": true, "bot_token": "...", "require_pairing": true },
  "qq": { "enabled": false, "app_id": "", "app_secret": "", "sandbox": false, "require_pairing": true }
}
```

Changing `enabled`, credentials, or `qq.sandbox` requires a **daemon restart**. Changing `require_pairing` alone applies live after saving config in the WebUI (no restart).

## Shared gateway commands

In any connected chat (after pairing if required):

| Command | Description |
|---------|-------------|
| `/help` | List gateway commands |
| `/agent [provider]` | Start agent session |
| `/agents [provider]` | Set default agent for this chat |
| `/pwd`, `/cd`, `/ll`, `/mkdir` | Working directory |
| `/quit` | Stop active agent session |
| `/show-thinking`, `/hide-thinking` | Thinking output toggle |

Platform-specific UI (e.g. Feishu folder cards) is described in each platform guide and [usage.md](../usage.md).

## Configuration reference

Field-level defaults and JSON structure: [config.md](../config.md).

## Integrating a new platform

Use the full checklist so code, docs, MCP, and WebUI stay aligned:

- [platform-integration-checklist.md](../platform-integration-checklist.md) (English)
- [platform-integration-checklist.zh-CN.md](../platform-integration-checklist.zh-CN.md) (中文)
