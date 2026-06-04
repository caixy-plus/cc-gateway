# Feishu / Lark Bot Setup

Connect a Feishu (Lark) custom app bot to cc-gateway over **WebSocket long connection** (pbbp2). No public webhook URL is required on your machine.

## Prerequisites

- A Feishu or Lark tenant where you can create and install apps
- cc-gateway daemon installed and `cc-gateway init` completed (optional but recommended)
- Outbound HTTPS access to Feishu Open Platform APIs

## 1. Create an app on Feishu Open Platform

1. Open [Feishu Open Platform](https://open.feishu.cn/app) (international tenants: [Lark](https://open.larksuite.com/app)).
2. **Create custom app** → note **App ID** and **App Secret** (Credentials page).
3. **Capabilities** → enable **Bot**.
4. **Events & callbacks**:
   - Add event **`im.message.receive_v1`** (receive messages).
   - Subscription mode: **WebSocket** / long connection (not HTTP callback for cc-gateway).
5. **Permissions**: grant scopes needed for messaging (at minimum send/receive IM as required by your tenant policy). Common scopes include reading and sending messages in chats the bot joins.
6. **Version management** → create a version and **Publish** / submit for admin approval if required.
7. **Install app** to your workspace (or test tenant).

## 2. Configure cc-gateway

Edit `~/.cc-gateway/config.json` or use WebUI **Settings → Feishu**:

```json
{
  "feishu": {
    "enabled": true,
    "app_id": "cli_xxxxxxxx",
    "app_secret": "your_app_secret",
    "require_pairing": true
  },
  "default_dir": "/path/to/your/projects"
}
```

| Field | Description |
|-------|-------------|
| `enabled` | Start Feishu integration when the daemon runs |
| `app_id` | Feishu app ID (`cli_...`) |
| `app_secret` | App secret from the console |
| `require_pairing` | New chats must be approved in WebUI before use |

Environment variables are supported, e.g. `"app_id": "${FEISHU_APP_ID}"`.

## 3. Start and verify

```sh
cc-gateway start
cc-gateway log -f
```

Look for Feishu WebSocket connection logs. In WebUI **Platforms**, Feishu should show as connected when healthy.

## 4. Pairing (if `require_pairing` is true)

1. In Feishu, open a **private chat** or **group** with the bot and send any message.
2. The bot replies with a pairing code.
3. In WebUI → **Pairing**, approve the Feishu chat (match platform `feishu` and the chat id shown).
4. Send `/agent` to start a session, then chat normally.

## 5. Usage notes

- **`/ll`**: Sends an **interactive card** listing folders under `default_dir`; tap a button to `cd`.
- **`/cd`**: In Feishu, path changes are constrained under `default_dir` (cannot navigate above it).
- **Per-chat isolation**: Each Feishu chat (group or DM) has its own agent subprocess and channel session.
- **Cards**: Only Feishu supports interactive cards; other platforms use plain text for `/ll`.
- **MCP `send_file`**: PNG/JPG/GIF/WebP etc. are sent as **image messages** (inline preview) via Feishu image API; other types use file messages. Max image size **10 MB** per Feishu docs.

## Troubleshooting

| Symptom | Things to check |
|---------|-----------------|
| No events | Event `im.message.receive_v1` added; subscription is **WebSocket**; app installed to workspace |
| Auth errors | `app_id` / `app_secret`; clock skew; app not published |
| Bot silent after message | Pairing not approved; check WebUI Pairing queue |
| Cannot send in group | Bot added to group; message permissions; @ bot if required by your setup |

## References

- [Feishu Open Platform](https://open.feishu.cn/document/home/index)
- [Create app (console)](https://open.feishu.cn/app)
- [Lark (international console)](https://open.larksuite.com/app)
- [Bot WebSocket long connection](https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/server-side-sdk/golang-sdk-guide/preparations)
- [Card JSON v2](https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes)
- [Button component (V2)](https://open.feishu.cn/document/feishu-cards/card-json-v2-components/interactive-components/button)
