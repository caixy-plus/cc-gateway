# QQ Official Bot Setup

Connect a **QQ Open Platform** robot (QQ 频道 / 群 / 私聊官方机器人 API v2) via **WebSocket Gateway**. cc-gateway uses the gateway long-connection model suitable for a local daemon (no public webhook required).

> Official docs increasingly recommend webhooks for production at scale; cc-gateway currently implements **WebSocket Gateway** for simpler self-hosted deployment.

## Prerequisites

- QQ Open Platform developer account and an approved **bot** application
- `app_id` and `app_secret` (client secret) from the console
- cc-gateway daemon with outbound HTTPS to QQ API hosts
- Bot intents including **C2C** and **group @ message** events (cc-gateway subscribes with intent `GROUP_AND_C2C`)

## 1. Register on QQ Open Platform

1. Open [QQ Open Platform](https://q.qq.com/) (developer portal for QQ bots).
2. Create a **robot** application and complete any required review.
3. In the console, copy **AppID** and **AppSecret** (client secret).
4. Enable message capabilities your bot needs:
   - **C2C** (user ↔ bot private messages) — event `C2C_MESSAGE_CREATE`
   - **Group** — event `GROUP_AT_MESSAGE_CREATE` (user @ bot in group)
5. For initial testing, you may use the **sandbox** environment (`sandbox: true` in config); switch to production API when ready.

## 2. Configure cc-gateway

Edit `~/.cc-gateway/config.json` or WebUI **Settings → QQ**:

```json
{
  "qq": {
    "enabled": true,
    "app_id": "102xxxxxx",
    "app_secret": "your_client_secret",
    "sandbox": false,
    "require_pairing": true
  }
}
```

| Field | Description |
|-------|-------------|
| `enabled` | Start QQ integration when the daemon runs |
| `app_id` | QQ bot AppID |
| `app_secret` | Client secret from the console |
| `sandbox` | `true` → `https://sandbox.api.sandbox.qq.com`; `false` → production `https://api.sgroup.qq.com` |
| `require_pairing` | New channels must be approved in WebUI |

Environment variables: `"app_id": "${QQ_APP_ID}"`, `"app_secret": "${QQ_APP_SECRET}"`.

## 3. Start and verify

```sh
cc-gateway restart   # required after enabling or changing credentials
cc-gateway log -f
```

Look for `[QQ] Gateway connected` in logs. WebUI **Platforms** should show QQ when connected.

**Auth flow (automatic):** daemon calls `https://bots.qq.com/app/getAppAccessToken`, then connects to the gateway URL from `GET /gateway/bot` with `Authorization: QQBot <token>`.

## 4. Pairing (if `require_pairing` is true)

Channel ids used internally:

| Chat type | Internal channel id | How to talk |
|-----------|---------------------|-------------|
| C2C (private) | `u:{user_openid}` | DM the bot |
| Group | `g:{group_openid}` | @ the bot in a group |

1. Send any message (group: **@ bot**).
2. Bot replies with pairing code.
3. WebUI → **Pairing** → approve platform `qq`.
4. `/agent` then chat.

## 5. Usage notes

- **`/ll`**, **`/agents`**: plain text lists (no QQ message cards yet).
- **Permission prompts**: text-only (no inline approve buttons like Telegram).
- **MCP `send_file`**: supported via rich media (`msg_type` 7). **Private (C2C):** images, video, voice, and generic files. **Group:** images, video, and voice only — PDF/zip etc. must be sent in private chat. Uses upload `POST …/files` then send with `media.file_info`.
- **Restart** after changing `app_id`, `app_secret`, `sandbox`, or `enabled`.

## Troubleshooting

| Symptom | Things to check |
|---------|-----------------|
| Token / gateway errors | `app_id` / `app_secret`; sandbox vs production mismatch |
| No C2C events | C2C intent / permission enabled in console |
| No group events | Bot in group; message must @ bot; group intent enabled |
| Silent bot | Pairing not approved; daemon restarted after config change |
| Sandbox vs prod | Set `sandbox` to match the credentials environment |

## API endpoints (reference)

| Purpose | URL |
|---------|-----|
| Access token | `POST https://bots.qq.com/app/getAppAccessToken` |
| Production API | `https://api.sgroup.qq.com` |
| Sandbox API | `https://sandbox.api.sandbox.qq.com` |

## References

- [QQ Open Platform (console)](https://q.qq.com/)
- [QQ Bot API v2 (wiki)](https://bot.q.qq.com/wiki/develop/api-v2/)
- [WebSocket gateway (opcodes, Identify/Resume)](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/reference.html)
- [Events and intents](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/event-emit.html)
- [Get WSS endpoint](https://bot.q.qq.com/wiki/develop/api-v2/openapi/wss/url_get.html)
- [Send/receive messages](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/send-receive/)
- [Rich media messages](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/send-receive/rich-media.html) (needed to implement MCP `send_file`)
