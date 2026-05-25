# 配置说明

cc-gateway 使用 JSON 配置文件，存储在 `~/.cc-gateway/config.json`。

所有字符串值均支持 `${VAR_NAME}` 环境变量替换。

## 示例

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

## 字段说明

### `log`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `level` | string | `"info"` | 日志级别: trace, debug, info, warn, error |
| `file` | string | `"~/.cc-gateway/logs/gateway.log"` | 日志文件路径 |

### 顶层字段

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `port` | u16 | `17534` | 守护进程绑定的本地端口，用于保证单实例运行 |
| `default_dir` | string | `"~"` | 网关会话的默认工作目录 |
| `show_thinking` | bool | `false` | 是否在输出中显示 Claude 的 Thinking 块 |
| `media_retention_days` | u64 | `30` | 下载的媒体文件保留天数 |

> **注意:** 守护进程会同时启动所有 `enabled` 为 `true` 的平台。你可以同时启用飞书和 Telegram，两者会并发运行。

### `claude`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `cli_path` | string | `"claude"` | Claude Code CLI 二进制路径 |
| `default_args` | string | `"--dangerously-skip-permissions"` | 每次启动会话时传递给 Claude CLI 的默认参数 |

你可以通过 `/claude <args>` 为每个会话覆盖或追加参数。

### `telegram`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `enabled` | bool | `false` | 启用 Telegram 机器人 |
| `bot_token` | string | `"${TELEGRAM_BOT_TOKEN}"` | Telegram Bot API token |
| `allow_from` | string | `"*"` | 允许的用户 ID 或用户名，逗号分隔; `"*"` = 允许所有 |
| `webhook_url` | string | `""` | Telegram Bot API 的 Webhook URL (留空则使用长轮询) |

### `feishu`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | 启用飞书机器人 |
| `app_id` | string | `"${FEISHU_APP_ID}"` | 飞书应用 ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | 飞书应用密钥 |
| `allow_from` | string | `"*"` | 允许的用户 open_id，逗号分隔; `"*"` = 允许所有 |
| `encrypt_key` | string | `""` | 事件加密密钥 (可选) |
| `mode` | string | `"websocket"` | 连接模式: `"websocket"` 或 `"webhook"` |
| `webhook_bind` | string | `"0.0.0.0:3000"` | Webhook 服务器绑定地址 |

`default_dir` 决定:
- `/ll` 在飞书交互卡片中列出哪个目录
- 飞书模式下 `/cd ..` 的上限 (无法导航到此目录之上)

## Telegram 设置

1. 在 Telegram 上联系 [@BotFather](https://t.me/BotFather) 创建新机器人
2. 将 bot token 复制到配置中的 `telegram.bot_token`
3. 将 `telegram.enabled` 设为 `true`
4. 可选: 设置 `telegram.allow_from` 限制哪些用户可以与机器人交互

## 飞书设置

1. 前往 [飞书开放平台](https://open.feishu.cn) 创建应用
2. 启用 "机器人" 能力
3. 添加 `im.message.receive_v1` 事件，选择 WebSocket 长连接模式
4. 将 `app_id` 和 `app_secret` 复制到配置中
5. 将应用安装到你的工作区
