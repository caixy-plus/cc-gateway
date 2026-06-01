# 配置说明

cc-gateway 使用 JSON 配置文件，路径为 `~/.cc-gateway/config.json`。

所有字符串值均支持 `${VAR_NAME}` 环境变量替换。

**各平台机器人分步配置：** 请参阅 [docs/bots/README.zh-CN.md](bots/README.zh-CN.md)。

## 示例

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
  "feishu": {
    "enabled": true,
    "app_id": "${FEISHU_APP_ID}",
    "app_secret": "${FEISHU_APP_SECRET}",
    "require_pairing": true
  },
  "telegram": {
    "enabled": false,
    "bot_token": "${TELEGRAM_BOT_TOKEN}",
    "require_pairing": true
  },
  "qq": {
    "enabled": false,
    "app_id": "${QQ_APP_ID}",
    "app_secret": "${QQ_APP_SECRET}",
    "sandbox": false,
    "require_pairing": true
  },
  "default_dir": "~/Workspace",
  "show_thinking": false,
  "port": 17534,
  "bind_address": "127.0.0.1"
}
```

可运行 `cc-gateway init` 使用向导，或在 WebUI **设置** 中编辑。

## 字段说明

### `log`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `level` | string | `"info"` | 日志级别 |
| `file` | string | `"~/.cc-gateway/logs/gateway.log"` | 日志文件路径 |
| `max_lines` | usize | `100000` | 日志保留最大行数 |
| `max_size_mb` | usize | `50` | 日志文件大小上限（MB） |

### 顶层字段

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `port` | u16 | `17534` | HTTP 端口（WebUI + 单实例锁） |
| `bind_address` | string | `"127.0.0.1"` | 监听地址（`0.0.0.0` 允许局域网） |
| `allowed_ips` | string[] | `[]` | 可选 IP/CIDR 白名单 |
| `webui_token` | string? | — | 可选 WebUI 访问令牌 |
| `default_dir` | string | `"~"` | 默认工作目录 |
| `show_thinking` | bool | `false` | 是否显示 Thinking 块 |
| `media_retention_days` | u64 | `30` | 媒体文件保留天数 |
| `session_retention_per_channel` | u64 | `30` | 每频道保留的智能体会话数（10–100） |

> **注意：** 守护进程会同时启动所有 `enabled: true` 的平台（飞书、Telegram、QQ 可任意组合）。

### `agent`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `default` | string | `"claude"` | `/agent` 未指定时的默认智能体 |
| `<provider>` | object | — | 各 provider 配置（`claude`、`cursor`、`pi`、`codewhale`、`opencode` 等） |

每个 provider 配置：

| 字段 | 类型 | 说明 |
|-------|------|------|
| `enabled` | bool | 是否在 `/agents` 与 init 中可用 |
| `cli_path` | string | CLI 路径 |
| `default_args` | string | 启动默认参数 |
| `mode` | string | 模式（若 provider 支持） |
| `permission` | string | `prompt` / `allow` / `deny` |

会话级覆盖：`/agent [provider] <额外参数>`。

### `feishu`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `enabled` | bool | 模板默认 `true`；`init` 前运行时默认 `false` | 启用飞书机器人 |
| `app_id` | string | `"${FEISHU_APP_ID}"` | 飞书 App ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | 飞书 App Secret |
| `require_pairing` | bool | `true` | 新聊天须 WebUI 配对 |

**配置指南：** [bots/feishu.zh-CN.md](bots/feishu.zh-CN.md)

### `telegram`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `enabled` | bool | `false` | 启用 Telegram |
| `bot_token` | string | `"${TELEGRAM_BOT_TOKEN}"` | BotFather Token |
| `require_pairing` | bool | `true` | 新聊天须 WebUI 配对 |

仅支持 **长轮询**（`getUpdates`）。**配置指南：** [bots/telegram.zh-CN.md](bots/telegram.zh-CN.md)

### `qq`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `enabled` | bool | `false` | 启用 QQ 官方机器人 |
| `app_id` | string | `"${QQ_APP_ID}"` | 机器人 AppID |
| `app_secret` | string | `"${QQ_APP_SECRET}"` | 客户端密钥 |
| `sandbox` | bool | `false` | `true` 使用沙箱 API |
| `require_pairing` | bool | `true` | 新频道须 WebUI 配对 |

使用 **WebSocket Gateway**（OpenAPI v2）。**配置指南：** [bots/qq.zh-CN.md](bots/qq.zh-CN.md)

### `default_dir`

- `/ll` 列出的目录根路径（飞书为卡片，其他平台为文本列表）。
- 飞书模式下 `/cd ..` 不能超过 `default_dir` 上级。

## 重启与热更新

| 变更项 | 生效方式 |
|--------|----------|
| 各平台凭证、`enabled`、`qq.sandbox` | 需 **重启守护进程** |
| 各平台 `require_pairing` | WebUI 保存后 **立即生效** |
| `port`、`bind_address`、`agent`、`log` 等 | 需 **重启守护进程** |

## 平台配置指南索引

| 平台 | 文档 |
|------|------|
| 飞书 / Lark | [bots/feishu.zh-CN.md](bots/feishu.zh-CN.md) |
| Telegram | [bots/telegram.zh-CN.md](bots/telegram.zh-CN.md) |
| QQ | [bots/qq.zh-CN.md](bots/qq.zh-CN.md) |
| 总览与配对 | [bots/README.zh-CN.md](bots/README.zh-CN.md) |
