# Configuration

cc-gateway uses JSON configuration stored at `~/.cc-gateway/config.json`.

All string values support `${VAR_NAME}` environment variable substitution.

**Per-platform bot setup (step-by-step):** see [docs/bots/README.md](bots/README.md).

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
      "enabled": true,
      "cli_path": "claude",
      "default_args": "--dangerously-skip-permissions"
    },
    "cursor": {
      "enabled": false,
      "cli_path": "agent",
      "default_args": ""
    }
  },
  "platforms": {
    "feishu": {
      "enabled": true,
      "app_id": "${FEISHU_APP_ID}",
      "app_secret": "${FEISHU_APP_SECRET}",
      "require_pairing": true
    },
    "telegram": {
      "enabled": false,
      "bot_token": "${TELEGRAM_BOT_TOKEN}",
      "proxy": "",
      "require_pairing": true
    },
    "qq": {
      "enabled": false,
      "app_id": "${QQ_APP_ID}",
      "app_secret": "${QQ_APP_SECRET}",
      "sandbox": false,
      "require_pairing": true
    }
  },
  "default_dir": "~/Workspace",
  "show_thinking": false,
  "port": 17534,
  "bind_address": "127.0.0.1"
}
```

Run `cc-gateway init` for an interactive wizard, or edit via WebUI **Settings**.

## Fields

### `log`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `level` | string | `"info"` | Log level: trace, debug, info, warn, error |
| `file` | string | `"~/.cc-gateway/logs/gateway.log"` | Log file path |
| `max_lines` | usize | `100000` | Max lines retained in the log file |
| `max_size_mb` | usize | `50` | Max log file size (MB) before rotation |

### Top-level fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `port` | u16 | `17534` | HTTP port (WebUI + single-instance lock) |
| `bind_address` | string | `"127.0.0.1"` | Bind address (`0.0.0.0` for LAN) |
| `allowed_ips` | string[] | `[]` | Optional CIDR allowlist (empty = no IP filter) |
| `webui_token` | string? | — | Optional WebUI access token |
| `default_dir` | string | `"~"` | Default working directory for sessions |
| `show_thinking` | bool | `false` | Show agent Thinking blocks in output |
| `media_retention_days` | u64 | `30` | Days to keep downloaded media |
| `session_retention_per_channel` | u64 | `30` | Max agent sessions kept per channel (10–100) |

> **Note:** The daemon starts **all platforms whose `enabled` flag is `true`** at once (Feishu, Telegram, QQ, or any combination).

### `agent`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `default` | string | `"claude"` | Default provider for `/agent` when omitted |
| `<provider>` | object | — | Per-provider profile (`claude`, `cursor`, `pi`, `opencode`, …) |

Each provider profile:

| Field | Type | Description |
|-------|------|-------------|
| `enabled` | bool | Whether the provider appears in `/agents` and init |
| `cli_path` | string | CLI binary (defaults per provider) |
| `default_args` | string | Args passed on session start |
| `mode` | string | Provider mode when supported |
| `permission` | string | `prompt`, `allow`, or `deny` |

Override per session: `/agent [provider] <extra args>`.

### `platforms`

Object keyed by platform id (`feishu`, `telegram`, `qq`, …). Legacy top-level `feishu` / `telegram` / `qq` keys are upgraded automatically on load. WebUI **Settings** and `GET /api/platforms` use the registry field schema for each integrated platform.

#### `platforms.feishu`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` in file template; `false` until `init` | Enable Feishu bot |
| `app_id` | string | `"${FEISHU_APP_ID}"` | Feishu app ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | Feishu app secret |
| `require_pairing` | bool | `true` | Require WebUI approval for new chats |

**Setup guide:** [bots/feishu.md](bots/feishu.md)

#### `platforms.telegram`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable Telegram bot |
| `bot_token` | string | `"${TELEGRAM_BOT_TOKEN}"` | BotFather HTTP API token |
| `require_pairing` | bool | `true` | Require WebUI approval for new chats |

Uses **long-polling** (`getUpdates`) only. **Setup guide:** [bots/telegram.md](bots/telegram.md)

#### `platforms.qq`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable QQ official bot |
| `app_id` | string | `"${QQ_APP_ID}"` | QQ bot AppID |
| `app_secret` | string | `"${QQ_APP_SECRET}"` | Client secret |
| `sandbox` | bool | `false` | Use sandbox API hosts when `true` |
| `require_pairing` | bool | `true` | Require WebUI approval for new channels |

Uses **WebSocket Gateway** (OpenAPI v2). **Setup guide:** [bots/qq.md](bots/qq.md)

### `default_dir`

- Root for `/ll` listings (Feishu cards list under this path; other platforms use text lists).
- Upper bound for `/cd ..` in Feishu (cannot navigate above `default_dir`).

## Restart vs live config

| Change | Effect |
|--------|--------|
| `feishu` / `telegram` / `qq` credentials, `enabled`, `qq.sandbox` | **Restart daemon** required |
| `require_pairing` on any platform | Applied **live** when saved from WebUI |
| `port`, `bind_address`, `agent`, `log`, … | **Restart daemon** required |

## Platform setup (quick links)

| Platform | Guide |
|----------|-------|
| Feishu / Lark | [bots/feishu.md](bots/feishu.md) |
| Telegram | [bots/telegram.md](bots/telegram.md) |
| QQ | [bots/qq.md](bots/qq.md) |
| Overview + pairing | [bots/README.md](bots/README.md) |
