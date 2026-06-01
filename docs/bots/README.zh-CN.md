# 各平台机器人配置总览

cc-gateway 可同时接入多个聊天机器人；在 `~/.cc-gateway/config.json` 里将对应平台的 `enabled` 设为 `true` 即可，守护进程会并行运行所有已启用平台。

| 平台 | 指南 | 连接方式 | MCP `send_file` | 配置节 |
|------|------|----------|-----------------|--------|
| 飞书 / Lark | [feishu.zh-CN.md](feishu.zh-CN.md) | WebSocket（pbbp2） | 支持 | `feishu` |
| Telegram | [telegram.zh-CN.md](telegram.zh-CN.md) | HTTP 长轮询（`getUpdates`） | 支持 | `telegram` |
| QQ 官方机器人 | [qq.zh-CN.md](qq.zh-CN.md) | WebSocket Gateway（OpenAPI v2） | 支持（群聊仅富媒体） | `qq` |

English: [README.md](README.md)

## 通用快速上手

1. **安装** cc-gateway，运行 `cc-gateway init`（或直接编辑 `~/.cc-gateway/config.json` / WebUI 设置）。
2. 按各平台指南填写 **机器人凭证**。
3. **启动守护进程：** `cc-gateway start`（修改凭证后需 `cc-gateway restart`）。
4. 浏览器打开 WebUI：`http://127.0.0.1:<port>/`（默认端口 `17534`），查看平台状态与配对。

## 配对放行（建议开启）

各平台默认 `require_pairing: true`。**新**聊天须先在 WebUI **配对** 页放行，消息才会进入智能体。

1. 用户向机器人发任意消息 → 机器人回复配对码。
2. 管理员打开 WebUI → 配对 → 批准（或输入配对码）。
3. 用户即可使用 `/agent`、`/help` 及正常对话。

可在配置或 WebUI 中关闭 `require_pairing`（仅建议用于私有/测试机器人）。

## 同时启用多个平台

守护进程会并行启动所有 `"enabled": true` 的平台，例如：

```json
{
  "feishu": { "enabled": true, "app_id": "...", "app_secret": "...", "require_pairing": true },
  "telegram": { "enabled": true, "bot_token": "...", "require_pairing": true },
  "qq": { "enabled": false, "app_id": "", "app_secret": "", "sandbox": false, "require_pairing": true }
}
```

修改 `enabled`、凭证或 `qq.sandbox` 需要 **重启守护进程**。仅修改 `require_pairing` 时，在 WebUI 保存后即可生效（无需重启）。

## 通用网关命令

配对通过后，各平台均可使用：

| 命令 | 说明 |
|------|------|
| `/help` | 命令列表 |
| `/agent [provider]` | 启动智能体会话 |
| `/agents [provider]` | 设置本聊天默认智能体 |
| `/pwd`、`/cd`、`/ll`、`/mkdir` | 工作目录 |
| `/quit` | 结束当前智能体会话 |
| `/show-thinking`、`/hide-thinking` | Thinking 显示开关 |

各平台特有交互（如飞书目录卡片）见对应指南与 [usage.zh-CN.md](../usage.zh-CN.md)。

## 配置字段说明

完整 JSON 字段与默认值：[config.zh-CN.md](../config.zh-CN.md)。

**接入新平台？** 请使用 [platform-integration-checklist.zh-CN.md](../platform-integration-checklist.zh-CN.md)，避免遗漏代码、文档、MCP 与 WebUI。
