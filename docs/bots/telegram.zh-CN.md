# Telegram 机器人配置

通过 Bot API **长轮询**（`getUpdates`）接入 Telegram。cc-gateway **不支持** Telegram Webhook，只需本机能访问 `api.telegram.org`。

## 前置条件

- Telegram 账号
- 已安装 cc-gateway
- 可访问 `api.telegram.org`

## 1. 使用 BotFather 创建机器人

1. 在 Telegram 中打开 [@BotFather](https://t.me/BotFather)。
2. 发送 `/newbot`，按提示设置名称与以 `bot` 结尾的用户名。
3. 复制返回的 **HTTP API Token**（形如 `123456789:ABC...`）。
4. 可选设置：
   - `/setprivacy` — **群聊** 中若需接收所有消息，可能需关闭隐私模式；否则往往需 @ 机器人才会投递消息。
   - `/setcommands` — 可选；网关命令由 cc-gateway 解析，不依赖 BotFather 命令表。

## 2. 配置 cc-gateway

编辑 `~/.cc-gateway/config.json` 或 WebUI **设置 → Telegram**：

```json
{
  "telegram": {
    "enabled": true,
    "bot_token": "123456789:AA...你的Token",
    "require_pairing": true
  }
}
```

| 字段 | 说明 |
|------|------|
| `enabled` | 是否启动 Telegram 集成 |
| `bot_token` | BotFather 提供的 Token |
| `require_pairing` | 新聊天是否须 WebUI 配对 |

可使用环境变量：`"bot_token": "${TELEGRAM_BOT_TOKEN}"`。

**安全：** 勿将真实 Token 提交到 Git；优先使用环境变量。

## 3. 启动与验证

```sh
cc-gateway start
cc-gateway log -f
```

WebUI **平台** 页应显示 Telegram。在 Telegram 私聊机器人发送 `/start` 或任意消息测试。

## 4. 配对放行（`require_pairing: true` 时）

1. 建议先在 **私聊** 中测试。
2. 发送任意消息 → 机器人回复配对码。
3. WebUI → **配对** → 批准（平台 `telegram`）。
4. 发送 `/agent` 后再发对话内容。

## 5. 使用说明

- **仅长轮询** — 无 `webhook_url` 配置项。
- **内联按钮** — 部分权限确认支持 Allow / Deny 按钮。
- **`/ll`** — 纯文本目录列表，配合 `/cd` 使用。
- **会话隔离** — 每个 Telegram `chat_id` 独立会话状态。

## 故障排查

| 现象 | 排查项 |
|------|--------|
| `401 Unauthorized` | Token 错误或已撤销 |
| 无消息 | 守护进程是否运行；`telegram.enabled`；网络 |
| 群内无响应 | 隐私模式；是否入群；尝试 @ 机器人 |
| 无智能体回复 | 是否已配对；是否已 `/agent` |

## 参考

- [Telegram Bot API](https://core.telegram.org/bots/api)
- [BotFather（创建机器人）](https://t.me/BotFather)
- [`getUpdates`（长轮询）](https://core.telegram.org/bots/api#getupdates)
- [`sendMessage`](https://core.telegram.org/bots/api#sendmessage)
