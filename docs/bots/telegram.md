# Telegram Bot Setup

Connect a Telegram bot via the **Bot API long-polling** loop (`getUpdates`). cc-gateway does not implement Telegram webhooks; your host only needs outbound HTTPS to `api.telegram.org`.

## Prerequisites

- A Telegram account
- cc-gateway daemon installed
- Outbound access to `api.telegram.org`

## 1. Create a bot with BotFather

1. In Telegram, open [@BotFather](https://t.me/BotFather).
2. Send `/newbot` and follow prompts (display name + username ending in `bot`).
3. Copy the **HTTP API token** BotFather returns (format `123456789:ABCdef...`).
4. Optional BotFather settings:
   - `/setprivacy` — for **group** use, you may need to disable privacy mode so the bot sees all messages, or only respond when @mentioned (cc-gateway handles normal messages in private chats; in groups, behavior depends on Telegram privacy settings).
   - `/setcommands` — optional; gateway commands are handled by cc-gateway, not BotFather command list.

## 2. Configure cc-gateway

Edit `~/.cc-gateway/config.json` or WebUI **Settings → Telegram**:

```json
{
  "telegram": {
    "enabled": true,
    "bot_token": "123456789:AA...your_token",
    "require_pairing": true
  }
}
```

| Field | Description |
|-------|-------------|
| `enabled` | Start Telegram integration when the daemon runs |
| `bot_token` | Token from BotFather |
| `require_pairing` | New chats must be approved in WebUI |

Use env substitution: `"bot_token": "${TELEGRAM_BOT_TOKEN}"`.

**Security:** Never commit real tokens. Prefer environment variables or a restricted `config.json` file mode.

## 3. Start and verify

```sh
cc-gateway start
cc-gateway log -f
```

WebUI **Platforms** should list Telegram when polling is active. Send `/start` or any message to your bot in Telegram.

## 4. Pairing (if `require_pairing` is true)

1. Open a **private chat** with your bot (recommended for first test).
2. Send any message → bot replies with a pairing code.
3. WebUI → **Pairing** → approve (platform `telegram`, chat id is the numeric Telegram chat id).
4. Run `/agent` then send prompts.

## 5. Usage notes

- **Long-polling only** — no `webhook_url` setting; the daemon pulls updates continuously.
- **Inline keyboards** — permission / confirm prompts may show **Allow / Deny** buttons where supported.
- **`/ll`** — plain-text folder list (reply with path or use `/cd`); no interactive cards.
- **Per-chat isolation** — each Telegram chat id has its own agent session state.
- **MCP `send_file`**: image files use Bot API **`sendPhoto`** (inline preview); other files use `sendDocument`.

## Troubleshooting

| Symptom | Things to check |
|---------|-----------------|
| `401 Unauthorized` | Wrong or revoked `bot_token` |
| No updates | Daemon running; `telegram.enabled: true`; network to Telegram |
| Group ignores bot | Privacy mode; bot added to group; try @mentioning the bot |
| No agent replies | Pairing not approved; run `/agent` first |

## References

- [Telegram Bot API](https://core.telegram.org/bots/api)
- [BotFather (create bot)](https://t.me/BotFather)
- [`getUpdates` (long polling)](https://core.telegram.org/bots/api#getupdates)
- [`sendMessage`](https://core.telegram.org/bots/api#sendmessage)
