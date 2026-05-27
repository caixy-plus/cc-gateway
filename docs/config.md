# Configuration

cc-gateway uses JSON configuration stored at `~/.cc-gateway/config.json`.

All string values support `${VAR_NAME}` environment variable substitution.

## Example

```json
{
  "log": {
    "level": "info",
    "file": "~/.cc-gateway/logs/gateway.log"
  },
  "agent": {
    "default": "claude",
    "claude": {
      "cli_path": "claude",
      "default_args": "--dangerously-skip-permissions",
      "mode": "agent",
      "permission": "prompt"
    },
    "cursor": {
      "cli_path": "agent",
      "default_args": "",
      "mode": "agent",
      "permission": "prompt"
    }
  },
  "feishu": {
    "enabled": true,
    "app_id": "${FEISHU_APP_ID}",
    "app_secret": "${FEISHU_APP_SECRET}",
    "allow_from": "*",
    "encrypt_key": "",
    "mode": "websocket",
    "webhook_bind": "0.0.0.0:3000"
  },
  "telegram": {
    "enabled": false,
    "bot_token": "${TELEGRAM_BOT_TOKEN}",
    "allow_from": "*",
    "webhook_url": ""
  },
  "default_dir": "~/Workspace",
  "port": 17534
}
```

## Fields

### `log`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `level` | string | `"info"` | Log level: trace, debug, info, warn, error |
| `file` | string | `"~/.cc-gateway/logs/gateway.log"` | Log file path |

### Top-level fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | u16 | `17534` | Local port bound by the daemon to enforce a single instance |
| `default_dir` | string | `"~"` | Default working directory for gateway sessions |
| `show_thinking` | bool | `false` | Display Claude's Thinking blocks in output |
| `media_retention_days` | u64 | `30` | Days to retain downloaded media files |

> **Note:** The daemon starts **all platforms whose `enabled` flag is `true`** simultaneously. You can run Feishu and Telegram at the same time by enabling both.

### `agent`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default` | string | `"claude"` | Default provider used by `/agent` when no provider is specified |
| `claude` | object |  | Provider profile for `claude` |
| `cursor` | object |  | Provider profile for `cursor` |

Each provider profile supports:

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cli_path` | string | `"claude"` / `"agent"` | CLI binary path for the provider |
| `default_args` | string | `""` | Default args passed to the provider on session start |
| `mode` | string | `"agent"` | Provider mode (passed to the provider if supported) |
| `permission` | string | `"prompt"` | Permission policy: `prompt`, `allow`, `deny` |

You can override or append arguments per session via `/agent [provider] <args>`.

### `telegram`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable Telegram bot |
| `bot_token` | string | `"${TELEGRAM_BOT_TOKEN}"` | Telegram Bot API token |
| `allow_from` | string | `"*"` | Allowed user IDs or usernames, comma-separated; `"*"` = all |
| `webhook_url` | string | `""` | Webhook URL for Telegram Bot API (empty = long-polling) |

### `feishu`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable Feishu bot |
| `app_id` | string | `"${FEISHU_APP_ID}"` | Feishu app ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | Feishu app secret |
| `allow_from` | string | `"*"` | Allowed user open_ids, comma-separated; `"*"` = all |
| `encrypt_key` | string | `""` | Event encrypt key (optional) |
| `mode` | string | `"websocket"` | Connection mode: `"websocket"` or `"webhook"` |
| `webhook_bind` | string | `"0.0.0.0:3000"` | Bind address for webhook server |

`default_dir` determines:
- Which directory `/ll` lists in Feishu interactive cards
- The upper boundary for `/cd ..` in Feishu mode (cannot navigate above this directory)

## Telegram Setup

1. Message [@BotFather](https://t.me/BotFather) on Telegram and create a new bot
2. Copy the bot token to `telegram.bot_token` in your config
3. Set `platform` to `"telegram"` and `telegram.enabled` to `true`
4. Optionally set `telegram.allow_from` to restrict which users can interact with the bot

## Feishu Setup

1. Go to [Feishu Open Platform](https://open.feishu.cn) and create an app
2. Enable "Bot" capability
3. Add `im.message.receive_v1` event, select WebSocket long-connection mode
4. Copy `app_id` and `app_secret` to your config
5. Install the app to your workspace
