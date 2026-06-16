# Feishu / Lark Bot Setup

Connect a Feishu (Lark) custom app bot to cc-gateway over **WebSocket long connection** (pbbp2). The gateway connects outbound to Feishu, so your machine does **not** need a public IP, domain, or HTTP callback URL.

This guide follows Feishu's current Open Platform docs for message events, WebSocket event subscription, and IM message APIs. Console labels may differ slightly between Feishu CN and Lark international tenants, but the required app shape is the same.

## Prerequisites

- A Feishu or Lark tenant where you can create an **enterprise self-built / custom app** and install it to a workspace
- cc-gateway installed locally
- At least one enabled agent provider, usually configured by `cc-gateway init`
- Outbound network access to Feishu Open Platform / Lark Open Platform APIs

## 1. Install and initialize cc-gateway

Install cc-gateway first so you have the config directory and WebUI ready:

```sh
cc-gateway init
cc-gateway webui
```

`cc-gateway webui` starts the daemon if needed and opens the local WebUI. Keep the WebUI available for later pairing approval.

## 2. Create the Feishu / Lark app

1. Open [Feishu Open Platform](https://open.feishu.cn/app) (international tenants: [Lark Developer Console](https://open.larksuite.com/app)).
2. Create an **enterprise self-built app / custom app**. Do not create a mini app or webhook-only bot.
3. Open the app's **Credentials / Basic information** page and copy:
   - **App ID** (`cli_...`)
   - **App Secret**
4. Add or enable the **Bot** capability.
5. Configure the bot's availability / visible range so the target users or groups can add and message it.

## 3. Configure app permissions

Open **Permissions / Scopes** in the Feishu console, search for the permission names or IDs below, add them, then save.

Feishu also supports **Batch import/export permissions**. For the minimum required cc-gateway chat setup, open **Permissions / Scopes → Batch import/export permissions → Import**, replace the example with:

```json
{
  "scopes": {
    "tenant": [
      "im:message:send_as_bot",
      "im:message.p2p_msg:readonly",
      "im:message.group_at_msg:readonly"
    ]
  }
}
```

This JSON is intentionally minimal: it allows the bot to receive direct messages, receive group @mention messages, and send replies / interactive cards. If the console says a scope has been replaced by a newer equivalent in your tenant, choose the current replacement shown by Feishu.

Minimum for ordinary cc-gateway chat:

| Purpose | Permission / scope |
|---------|--------------------|
| Send replies, rich text, and interactive cards as the bot | `im:message:send_as_bot` or the broader `im:message` scope |
| Receive direct messages sent to the bot | `im:message.p2p_msg:readonly` |
| Receive group messages where users @mention the bot | `im:message.group_at_msg:readonly` |

Recommended when you use attachments or agent `send_file`:

| Purpose | Permission / scope |
|---------|--------------------|
| Upload and download message images/files | `im:resource` or the console's current "get/upload IM resources" scope |
| Receive all messages in groups without requiring @mention | `im:message.group_msg:readonly` (sensitive; only request if your tenant allows and you really need it) |

Notes:

- Feishu has deprecated some older message scopes. If your console shows both an old and a new scope, choose the current `:readonly` or broader replacement shown above.
- Interactive cards are not optional in cc-gateway. `/ll`, `/agents`, `/models`, `/agent-history`, permission allow/deny prompts, and picker/confirm prompts all use Feishu cards. Card delivery uses the message-send scope above; button clicks are configured in the event/callback step below.
- Permission batch import only imports API scopes. It does **not** subscribe events or card callbacks; still configure `im.message.receive_v1` and `card.action.trigger` in the next step.
- Updating app permissions does not take effect for installed users until you publish a new app version and it is approved/installed.

## 4. Configure event subscription

Open **Events & callbacks / Event subscription**:

1. Enable event subscription.
2. Choose **WebSocket / Long connection** as the receiving method. Do **not** configure HTTP callback URL for cc-gateway.
3. Add event **`im.message.receive_v1`** (Receive message).
4. Add card interaction callback **`card.action.trigger`** (Card action trigger / card callback). If your console separates "event subscription" and "card callback interaction", open the card callback section and enable the new card callback flow there.
5. Make sure card callbacks are also received by **WebSocket / Long connection**. Do not switch card callbacks to an HTTP callback URL; cc-gateway does not expose one.
6. Save the event settings.

cc-gateway obtains the WebSocket endpoint from Feishu using your App ID and App Secret. You do not paste a callback URL, verification token, or encrypt key into cc-gateway.

Why this matters: Feishu sends normal user messages as `im.message.receive_v1`, but interactive-card button clicks arrive separately as `card.action.trigger`. cc-gateway relies on those callbacks for `/ll` directory navigation, `/agents`, `/models`, `/agent-history`, permission **Allow / Deny** buttons, and other card-based choices. If `card.action.trigger` is missing, text chat can work while all card buttons appear to do nothing.

## 5. Publish and install the app

After changing bot capability, permissions, or event subscriptions:

1. Go to **Version management / Publish**.
2. Create a new version.
3. Submit it for administrator approval if your tenant requires approval.
4. Install or re-install the app to the target workspace.
5. Add the bot to the target group chats, or open a direct chat with the bot.

If you skip this step, the app may connect successfully but receive no message events.

## 6. Configure cc-gateway

Edit `~/.cc-gateway/config.json` or use WebUI **Settings → Feishu**:

```json
{
  "platforms": {
    "feishu": {
      "enabled": true,
      "app_id": "cli_xxxxxxxx",
      "app_secret": "your_app_secret",
      "require_pairing": true
    }
  },
  "default_dir": "/path/to/your/projects"
}
```

(Other top-level keys such as `agent`, `log`, and `port` are omitted here. Full layout: [config.md](../config.md).)

| Field | Description |
|-------|-------------|
| `enabled` | Start Feishu integration when the daemon runs |
| `app_id` | Feishu app ID (`cli_...`) |
| `app_secret` | App secret from the console |
| `require_pairing` | New chats must be approved in WebUI before use |

Environment variables are supported, e.g. `"app_id": "${FEISHU_APP_ID}"`.

Changing `enabled`, `app_id`, or `app_secret` requires a daemon restart. Changing `require_pairing` in WebUI applies live.

## 7. Start and verify

```sh
cc-gateway restart
cc-gateway status
cc-gateway log -f
```

Look for logs similar to:

- `Starting Feishu platform...`
- `Feishu WebSocket endpoint: ...`
- `Feishu WebSocket connected successfully`

In WebUI **Platforms**, Feishu should show as connected when healthy.

Then send a test message:

- Direct chat: send `hello` or `/help` to the bot.
- Group chat: add the bot to the group, then send `@bot /help` unless your app has a group-all-messages permission.

## 8. Pairing (if `require_pairing` is true)

1. In Feishu, open a **private chat** or **group** with the bot and send any message.
2. The bot replies with a pairing code.
3. In WebUI → **Pairing**, approve the Feishu chat (match platform `feishu` and the chat id shown).
4. Send `/agent` to start a session, then chat normally.

If the bot only replies with a pairing code, the Open Platform side is working; approve the chat before testing agent commands.

## 9. Supported message behavior

Inbound messages:

- Text and rich text are forwarded as user text.
- Images, files, audio, video/media, and rich-text images are downloaded into `~/.cc-gateway/media/` and forwarded to the agent as local file references.
- Empty or unsupported message types are ignored after acknowledging the Feishu event.

Outbound messages:

- Normal assistant output is sent as text/rich text.
- `/ll`, `/agents`, `/models`, `/agent-history`, permission prompts, and picker/confirm prompts use Feishu interactive cards.
- Card button clicks require the `card.action.trigger` callback configured over WebSocket / Long connection. cc-gateway updates cards in-place after clicks when Feishu returns the card context.
- MCP `send_file` sends images as Feishu image messages and other files as Feishu file messages.

- **`/ll`**: Sends an **interactive card** listing subfolders of the **current** work dir (starts at `default_dir`); tap a button to `cd`.
- **`/cd`**: Same home-directory bound as other platforms (`ensure_under_home`); not limited to `default_dir`.
- **Per-chat isolation**: Each Feishu chat (group or DM) has its own agent subprocess and channel session.
- **MCP `send_file` limits**: Image upload max is **10 MB**; file upload max is **30 MB** per Feishu docs.

## Troubleshooting

| Symptom | Things to check |
|---------|-----------------|
| WebUI shows Feishu disconnected | `app_id` / `app_secret`; outbound access to Feishu; daemon logs around `callback/ws/endpoint` |
| WebSocket connected but no incoming messages | Event `im.message.receive_v1` added; receiving method is **WebSocket**; permissions published; app installed to workspace |
| Direct chat does not trigger | Bot capability enabled; app visible to the user; `im:message.p2p_msg:readonly` or replacement scope approved |
| Group chat does not trigger | Bot added to group; user @mentions the bot; `im:message.group_at_msg:readonly` approved, or `im:message.group_msg:readonly` if you expect all group messages |
| Bot receives message but cannot reply | `im:message:send_as_bot` or `im:message`; bot still in the group; group allows bot messages |
| Card messages are not sent | `im:message:send_as_bot` or `im:message`; app version republished; daemon logs around `send_interactive_card` |
| Cards are sent but buttons do nothing | `card.action.trigger` added; card callback interaction enabled; card callbacks use **WebSocket / Long connection** rather than HTTP; app version republished; daemon logs show `card.action.trigger` |
| `/ll`, `/agents`, `/models`, or permission buttons do not respond | Same checks as card buttons; these features all depend on `card.action.trigger` |
| Attachments fail | IM resource upload/download scope; file size within Feishu limits; check `~/.cc-gateway/logs/gateway.log` |
| Bot only sends pairing code | Approve the chat in WebUI → Pairing, or set `require_pairing: false` for private/test bots |
| Config change has no effect | Restart after changing `enabled`, `app_id`, or `app_secret`; publish/install the Feishu app after console changes |

## References

- [Feishu Open Platform](https://open.feishu.cn/document/home/index)
- [Create app (console)](https://open.feishu.cn/app)
- [Lark (international console)](https://open.larksuite.com/app)
- [Receive message event (`im.message.receive_v1`)](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive)
- [Send message API](https://open.feishu.cn/document/server-docs/im-v1/message/create)
- [Receive events through WebSocket](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case)
- [Configure card interactions](https://open.feishu.cn/document/feishu-cards/configuring-card-interactions)
- [Handle card callbacks](https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/handle-card-callbacks)
- [Card callback communication (`card.action.trigger`)](https://open.feishu.cn/document/feishu-cards/card-callback-communication)
- [Receive callbacks through WebSocket](https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address)
- [Card JSON v2](https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes)
- [Button component (V2)](https://open.feishu.cn/document/feishu-cards/card-json-v2-components/interactive-components/button)
- [Upload image](https://open.feishu.cn/document/server-docs/im-v1/image/create)
- [Upload file](https://open.feishu.cn/document/server-docs/im-v1/file/create)
