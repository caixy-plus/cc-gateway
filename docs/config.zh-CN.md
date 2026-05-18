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
    "default_dir": "~/Workspace"
  }
}
```

## 字段说明

### `log`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `level` | string | `"info"` | 日志级别: trace, debug, info, warn, error |
| `file` | string | `"~/.cc-gateway/logs/gateway.log"` | 日志文件路径 |

### `claude`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `cli_path` | string | `"claude"` | Claude Code CLI 二进制路径 |
| `default_args` | string | `"--dangerously-skip-permissions"` | 每次启动会话时传递给 Claude CLI 的默认参数 |

你可以通过 `/claude <args>` 为每个会话覆盖或追加参数。

### `feishu`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | 启用飞书机器人 |
| `app_id` | string | `"${FEISHU_APP_ID}"` | 飞书应用 ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | 飞书应用密钥 |
| `allow_from` | string | `"*"` | 允许的用户 open_id，逗号分隔; `"*"` = 允许所有 |
| `encrypt_key` | string | `""` | 事件加密密钥 (可选) |
| `default_dir` | string | `"~/Workspace"` | 飞书 `/ll` 和 `/cd` 边界的默认目录 |

`default_dir` 决定:
- `/ll` 在飞书交互卡片中列出哪个目录
- 飞书模式下 `/cd ..` 的上限 (无法导航到此目录之上)

## 飞书设置

1. 前往 [飞书开放平台](https://open.feishu.cn) 创建应用
2. 启用 "机器人" 能力
3. 添加 `im.message.receive_v1` 事件，选择 WebSocket 长连接模式
4. 将 `app_id` 和 `app_secret` 复制到配置中
5. 将应用安装到你的工作区
