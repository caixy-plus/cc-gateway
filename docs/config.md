# Configuration

cc-gateway uses JSON configuration stored at `~/.cc-gateway/config.json`.

All string values support `${VAR_NAME}` environment variable substitution.

**Per-platform bot setup (step-by-step):** see [docs/bots/README.md](bots/README.md).

## Canonical structure (after load / auto-migration)

Top-level layout:

| Section | Purpose |
|---------|---------|
| `log` | Daemon log level, path, rotation |
| `agent` | `default` provider id + `providers` map (one object per **registered** id: `claude`, `codex`, `cursor`, `opencode`, `kimi`, `gemini`, `pi`, …) |
| `platforms` | Bot integrations (`feishu`, `telegram`, `qq`) — **not** top-level keys |
| `default_dir`, `show_thinking`, `media_retention_days`, `session_retention_per_channel` | Session / UI defaults |
| `port`, `bind_address`, `allowed_ips`, `webui_token` | HTTP / WebUI |

`agent` has two keys: `default` and `providers` (a map keyed by provider id). Legacy flat `agent.<id>` keys are migrated into `agent.providers` on load. After the first load on an old file, every catalog id is present on disk even if you never enabled that provider.

CLI binaries (`claude`, `codex-acp`, `agent`, `pi`, `opencode`, `kimi`, `gemini`) are **not** stored per profile — they come from the gateway [agent registry](adding-agent-provider.md). Profiles only hold `enabled`, `default_args`, `mode`, and `permission`.

**Codex** uses Zed's ACP adapter, not the raw `codex` CLI: `npm i -g @zed-industries/codex-acp`. Auth is shared with the Codex CLI (`codex login` or `OPENAI_API_KEY`).

## Example

Typical `~/.cc-gateway/config.json` after `init` or auto-migration:

```json
{
  "log": {
    "level": "info",
    "file": "~/.cc-gateway/logs/gateway.log",
    "max_lines": 100000,
    "max_size_mb": 50
  },
  "agent": {
    "default": "claude",
    "providers": {
      "claude": {
        "enabled": true,
        "default_args": "--dangerously-skip-permissions"
      },
      "codex": {
        "enabled": false
      },
      "cursor": {
        "enabled": false
      },
      "pi": {
        "enabled": false,
        "default_args": "--provider anthropic"
      },
      "opencode": {
        "enabled": false
      },
      "kimi": {
        "enabled": false
      },
      "gemini": {
        "enabled": false
      }
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
  "media_retention_days": 30,
  "session_retention_per_channel": 30,
  "port": 17534,
  "bind_address": "127.0.0.1",
  "allowed_ips": [],
  "webui_token": null
}
```

Run `cc-gateway init` for an interactive wizard, or edit via WebUI **Settings**.

On **daemon / WebUI load**, cc-gateway upgrades legacy on-disk shapes (top-level `feishu` → `platforms.feishu`, flat `agent.<id>` → `agent.providers.<id>`, legacy `agent.provider`, missing registry provider entries, etc.) and **writes `config.json` back** when the structure changed. Your field values are preserved; only layout is normalized. Invalid unknown keys under `agent` or `agent.providers` still fail load (see below).

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
| `providers` | object | Map of provider id → profile (one key per [registered](adding-agent-provider.md) id; auto-added on load if missing) |

Unknown keys directly under `agent` (only `default` and `providers` are allowed) or unknown ids under `agent.providers` (typos such as `"agnt"`) are **rejected at load time**. Allowed provider ids match the gateway agent catalog (see `GET /api/agents`).

Each provider profile (`agent.providers.<id>`):

| Field | Type | Description |
|-------|------|-------------|
| `enabled` | bool | Whether the provider appears in `/agents` and init |
| `default_args` | string? | Extra CLI args on session start (omitted when empty). Gateway-only `--yolo` maps to provider auto-approve semantics when supported |
| `mode` | string? | Provider mode when supported (omitted = registry default) |
| `permission` | string? | `prompt`, `allow`, or `deny` (omitted = registry default) |

CLI executable names are **not** configurable here; the gateway resolves them from the agent registry (`claude`, `codex-acp`, `agent`, `pi`, `opencode`, `kimi`, `gemini`).

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
| `proxy` | string | `""` | Optional HTTP/SOCKS proxy for Telegram Bot API only (e.g. `http://127.0.0.1:7890`) |
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

- **Initial** working directory when a channel/session starts (also the starting point for `/ll` before any `/cd`).
- **Not** the upper bound for `/cd`: all platforms enforce paths under the user **home directory** (`ensure_under_home`). Use `/cd_default` to reset to `default_dir`.

## Restart vs live config

| Change | Effect |
|--------|--------|
| `platforms.*` credentials, `enabled`, `qq.sandbox` | **Restart daemon** required |
| `require_pairing` on any platform | Applied **live** when saved from WebUI |
| `port`, `bind_address`, `agent`, `log`, … | **Restart daemon** required |

## Platform setup (quick links)

| Platform | Guide |
|----------|-------|
| Feishu / Lark | [bots/feishu.md](bots/feishu.md) |
| Telegram | [bots/telegram.md](bots/telegram.md) |
| QQ | [bots/qq.md](bots/qq.md) |
| Overview + pairing | [bots/README.md](bots/README.md) |
