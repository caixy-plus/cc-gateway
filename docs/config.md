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
  "claude": {
    "cli_path": "claude",
    "default_args": "--dangerously-skip-permissions"
  },
  "feishu": {
    "enabled": true,
    "app_id": "${FEISHU_APP_ID}",
    "app_secret": "${FEISHU_APP_SECRET}",
    "allow_from": "*",
    "encrypt_key": "",
    "default_dir": "~/Workspace"
  }
}
```

## Fields

### `log`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `level` | string | `"info"` | Log level: trace, debug, info, warn, error |
| `file` | string | `"~/.cc-gateway/logs/gateway.log"` | Log file path |

### `claude`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cli_path` | string | `"claude"` | Claude Code CLI binary path |
| `default_args` | string | `"--dangerously-skip-permissions"` | Default arguments passed to Claude CLI on every session start |

You can override or append arguments per session via `/claude <args>`.

### `feishu`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable Feishu bot |
| `app_id` | string | `"${FEISHU_APP_ID}"` | Feishu app ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | Feishu app secret |
| `allow_from` | string | `"*"` | Allowed user open_ids, comma-separated; `"*"` = all |
| `encrypt_key` | string | `""` | Event encrypt key (optional) |
| `default_dir` | string | `"~/Workspace"` | Default directory for Feishu `/ll` and `/cd` boundary |

`default_dir` determines:
- Which directory `/ll` lists in Feishu interactive cards
- The upper boundary for `/cd ..` in Feishu mode (cannot navigate above this directory)

## Feishu Setup

1. Go to [Feishu Open Platform](https://open.feishu.cn) and create an app
2. Enable "Bot" capability
3. Add `im.message.receive_v1` event, select WebSocket long-connection mode
4. Copy `app_id` and `app_secret` to your config
5. Install the app to your workspace
