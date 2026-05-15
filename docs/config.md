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
  "ai": {
    "enabled": false,
    "provider": "openai",
    "api_key": "${OPENAI_API_KEY}",
    "base_url": "https://api.openai.com/v1",
    "model": "gpt-4o-mini"
  },
  "claude": {
    "cli_path": "claude",
    "mode": "default",
    "model": "",
    "allowed_tools": ["Read", "Grep", "Glob", "Bash", "Edit", "Write"],
    "disallowed_tools": [],
    "system_prompt": "",
    "reasoning_effort": ""
  },
  "feishu": {
    "enabled": true,
    "app_id": "${FEISHU_APP_ID}",
    "app_secret": "${FEISHU_APP_SECRET}",
    "allow_from": "*",
    "encrypt_key": ""
  },
  "workspace": {
    "scan_dirs": ["~/Workspace", "~/Projects"],
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

### `ai`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | Enable AI intent recognition |
| `provider` | string | `"openai"` | AI provider name |
| `api_key` | string | `"${OPENAI_API_KEY}"` | API key (supports env var) |
| `base_url` | string | `"https://api.openai.com/v1"` | API base URL |
| `model` | string | `"gpt-4o-mini"` | Model name |

When `ai.enabled` is true, cc-gateway uses the configured AI to analyze user messages and automatically detect which local project they want to work on.

### `claude`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `cli_path` | string | `"claude"` | Claude Code CLI binary path |
| `mode` | string | `"default"` | Permission mode: default, acceptEdits, plan, auto, bypassPermissions |
| `model` | string | `""` | Default model (empty = use Claude's default) |
| `allowed_tools` | string[] | `["Read", "Grep", ...]` | Pre-approved tools |
| `disallowed_tools` | string[] | `[]` | Blocked tools |
| `system_prompt` | string | `""` | Custom system prompt |
| `reasoning_effort` | string | `""` | Reasoning effort: low, medium, high, max |

### `feishu`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | Enable Feishu bot |
| `app_id` | string | `"${FEISHU_APP_ID}"` | Feishu app ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | Feishu app secret |
| `allow_from` | string | `"*"` | Allowed user open_ids, comma-separated; "*" = all |
| `encrypt_key` | string | `""` | Event encrypt key (optional) |

### `workspace`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `scan_dirs` | string[] | `["~/Workspace", "~/Projects"]` | Directories to scan for projects |
| `default_dir` | string | `"~/Workspace"` | Default working directory |

## Feishu Setup

1. Go to [Feishu Open Platform](https://open.feishu.cn) and create an app
2. Enable "Bot" capability
3. Add `im.message.receive_v1` event, select WebSocket long-connection mode
4. Copy `app_id` and `app_secret` to your config
5. Install the app to your workspace
