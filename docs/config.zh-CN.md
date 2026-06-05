# 配置说明

cc-gateway 使用 JSON 配置文件，路径为 `~/.cc-gateway/config.json`。

所有字符串值均支持 `${VAR_NAME}` 环境变量替换。

**各平台机器人分步配置：** 请参阅 [docs/bots/README.zh-CN.md](bots/README.zh-CN.md)。

## 规范结构（加载 / 自动迁移后）

顶层分区：

| 区块 | 作用 |
|------|------|
| `log` | 守护进程日志级别、路径、轮转 |
| `agent` | `default` 默认 provider id + `providers` 映射（每个**已注册** id 一条：`claude`、`cursor`、`pi`、`opencode` …） |
| `platforms` | 聊天平台（`feishu`、`telegram`、`qq`）——**不要**再写在顶层 |
| `default_dir`、`show_thinking`、`media_retention_days`、`session_retention_per_channel` | 会话 / UI 默认项 |
| `port`、`bind_address`、`allowed_ips`、`webui_token` | HTTP / WebUI |

`agent` 含 `default` 与 `providers`（按 provider id 索引的对象）。旧版平铺的 `agent.<id>` 会在加载时迁入 `agent.providers`。旧文件首次加载后，目录里列出的每个 provider id 都会出现在磁盘上，即使你未启用该 provider。

各 profile **不保存** CLI 可执行文件名（由网关 [agent 注册表](adding-agent-provider.md) 解析为 `claude`、`agent`、`pi`、`opencode`）；profile 仅含 `enabled`、`default_args`、`mode`、`permission`。

## 示例

`init` 或自动迁移后的典型 `~/.cc-gateway/config.json`：

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
      "cursor": {
        "enabled": false
      },
      "pi": {
        "enabled": false,
        "default_args": "--provider anthropic"
      },
      "opencode": {
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

可运行 `cc-gateway init` 使用向导，或在 WebUI **设置** 中编辑。

**守护进程 / WebUI 加载**时，旧版布局（顶层 `feishu`、平铺 `agent.<id>` → `agent.providers.<id>`、遗留 `agent.provider`、缺少注册表 provider 条目等）会在内存中升级，并在结构变化时**自动写回** `config.json`（保留你已填写的有效字段，仅规范化布局）。`agent` 或 `agent.providers` 下未注册的 key 仍会加载失败（见下文）。

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
| `providers` | object | provider id → profile 映射（每个[已注册](adding-agent-provider.md) id 一条；加载时缺失会自动补全） |

`agent` 顶层只允许 `default` 与 `providers`；`agent.providers` 下不允许未注册的 provider id（拼写错误如 `"agnt"` 会在加载时报错）。允许的 id 与网关 agent 目录一致（见 `GET /api/agents`）。

每个 provider profile（`agent.providers.<id>`）：

| 字段 | 类型 | 说明 |
|-------|------|------|
| `enabled` | bool | 是否在 `/agents` 与 init 中可用 |
| `default_args` | string? | 会话启动附加 CLI 参数（空则省略）。网关专用 `--yolo` 在支持的 provider 上映射为自动放行语义 |
| `mode` | string? | 模式（省略则用注册表默认） |
| `permission` | string? | `prompt` / `allow` / `deny`（省略则用注册表默认） |

CLI 可执行文件名**不在**此配置；由 agent 注册表解析（`claude`、`agent`、`pi`、`opencode`）。

会话级覆盖：`/agent [provider] <额外参数>`。

### `platforms`

以平台 id 为键（`feishu`、`telegram`、`qq` 等）。旧版顶层 `feishu` / `telegram` / `qq` 会在加载时自动迁入 `platforms`。WebUI **设置** 与 `GET /api/platforms` 按注册表字段 schema 渲染。

#### `platforms.feishu`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `enabled` | bool | 模板默认 `true`；`init` 前运行时默认 `false` | 启用飞书机器人 |
| `app_id` | string | `"${FEISHU_APP_ID}"` | 飞书 App ID |
| `app_secret` | string | `"${FEISHU_APP_SECRET}"` | 飞书 App Secret |
| `require_pairing` | bool | `true` | 新聊天须 WebUI 配对 |

**配置指南：** [bots/feishu.zh-CN.md](bots/feishu.zh-CN.md)

#### `platforms.telegram`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `enabled` | bool | `false` | 启用 Telegram |
| `bot_token` | string | `"${TELEGRAM_BOT_TOKEN}"` | BotFather Token |
| `proxy` | string | `""` | 可选 HTTP/SOCKS 代理，仅 Telegram Bot API（如 `http://127.0.0.1:7890`） |
| `require_pairing` | bool | `true` | 新聊天须 WebUI 配对 |

仅支持 **长轮询**（`getUpdates`）。**配置指南：** [bots/telegram.zh-CN.md](bots/telegram.zh-CN.md)

#### `platforms.qq`

| 字段 | 类型 | 默认值 | 说明 |
|-------|------|---------|------|
| `enabled` | bool | `false` | 启用 QQ 官方机器人 |
| `app_id` | string | `"${QQ_APP_ID}"` | 机器人 AppID |
| `app_secret` | string | `"${QQ_APP_SECRET}"` | 客户端密钥 |
| `sandbox` | bool | `false` | `true` 使用沙箱 API |
| `require_pairing` | bool | `true` | 新频道须 WebUI 配对 |

使用 **WebSocket Gateway**（OpenAPI v2）。**配置指南：** [bots/qq.zh-CN.md](bots/qq.zh-CN.md)

### `default_dir`

- 频道/会话的**初始**工作目录（`/ll` 在首次 `/cd` 前从此处列出子目录）。
- **不是** `/cd` 的上界：全平台要求路径落在用户**主目录**内（`ensure_under_home`）。可用 `/cd_default` 重置为 `default_dir`。

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
