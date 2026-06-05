# QQ 官方机器人配置

接入 **QQ 开放平台** 官方机器人（OpenAPI v2），通过 **WebSocket Gateway** 长连接收消息。cc-gateway 当前仅支持 **C2C 私聊**；群 @ 消息会回复不支持提示。适合在本机运行守护进程，无需公网 Webhook。

> 官方文档在大规模生产环境更推荐 Webhook；cc-gateway 当前实现为 **WebSocket Gateway**，便于自建部署。

## 前置条件

- QQ 开放平台开发者账号及已创建的 **机器人** 应用
- 控制台中的 **AppID**、**AppSecret**（客户端密钥）
- cc-gateway 可访问 QQ API 域名
- 机器人已开通 **C2C** 私聊能力与事件 `C2C_MESSAGE_CREATE`

## 1. 在 QQ 开放平台创建机器人

1. 打开 [QQ 开放平台](https://q.qq.com/)。
2. 创建 **机器人** 应用并完成审核（按平台要求）。
3. 在应用详情复制 **AppID**、**AppSecret**。
4. 开通 **C2C 私聊** — 事件 `C2C_MESSAGE_CREATE`。
5. 测试阶段可在配置中设 `"sandbox": true` 使用沙箱 API；正式上线改为 `false`。

> **群聊：** `GROUP_AT_MESSAGE_CREATE` 不作为正常对话处理，群内发消息会看到 `qq.group_chat_unsupported`。

## 2. 配置 cc-gateway

编辑 `~/.cc-gateway/config.json` 或 WebUI **设置 → QQ**：

```json
{
  "platforms": {
    "qq": {
      "enabled": true,
      "app_id": "102xxxxxx",
      "app_secret": "你的AppSecret",
      "sandbox": false,
      "require_pairing": true
    }
  }
}
```

（完整 `config.json` 结构见 [config.zh-CN.md](../config.zh-CN.md)。）

| 字段 | 说明 |
|------|------|
| `enabled` | 是否启动 QQ 集成 |
| `app_id` | 机器人 AppID |
| `app_secret` | 客户端密钥 |
| `sandbox` | `true` 使用沙箱 `https://sandbox.api.sandbox.qq.com`；`false` 使用正式 `https://api.sgroup.qq.com` |
| `require_pairing` | 新频道是否须 WebUI 配对 |

环境变量示例：`"app_id": "${QQ_APP_ID}"`，`"app_secret": "${QQ_APP_SECRET}"`。

## 3. 启动与验证

```sh
cc-gateway restart   # 修改凭证或 enabled 后必须重启
cc-gateway log -f
```

日志中应出现 `[QQ] Gateway connected`。WebUI **平台** 页显示 QQ 在线即表示连接正常。

**鉴权流程（自动）：** 先请求 `https://bots.qq.com/app/getAppAccessToken` 获取 token，再调用 `GET /gateway/bot` 取得 WebSocket 地址并以 `Authorization: QQBot <token>` 连接。

## 4. 配对放行（`require_pairing: true` 时）

C2C 内部频道 ID：`u:{user_openid}`。

1. 私聊机器人发送任意消息。
2. 机器人回复配对码。
3. WebUI → **配对** → 批准平台 `qq`。
4. 发送 `/agent` 后开始对话。

## 5. 使用说明

- **`/ll`、`/agents`**：纯文本列表（暂无 QQ 消息卡片）。
- **权限确认**：文本提示为主（无 Telegram 式内联按钮）。
- **入站媒体（C2C）**：私聊中的图片/文件会下载到 `~/.cc-gateway/media/` 并在会话激活时转发给智能体。
- **MCP `send_file`（仅 C2C）**：富媒体（`msg_type` 7）。**内联图片** 使用 `file_type=1`（官方仅 **PNG/JPG**）。私聊支持图片、视频、语音及一般文件（WebP/GIF 等可能以文件 type 4 发送）。
- **修改** `app_id`、`app_secret`、`sandbox`、`enabled` 后需 **重启守护进程**。

## 故障排查

| 现象 | 排查项 |
|------|--------|
| Token / Gateway 失败 | AppID、AppSecret；沙箱与正式环境是否一致 |
| 无私聊消息 | 控制台是否开通 C2C 与对应事件 |
| 群消息无对话 | 预期行为 — 当前仅支持 C2C |
| 无响应 | 是否已配对；配置变更后是否 restart |
| 沙箱异常 | `sandbox` 是否与凭证环境一致 |

## API 地址（参考）

| 用途 | 地址 |
|------|------|
| 获取 access token | `POST https://bots.qq.com/app/getAppAccessToken` |
| 正式环境 API | `https://api.sgroup.qq.com` |
| 沙箱环境 API | `https://sandbox.api.sandbox.qq.com` |

## 参考

- [QQ 开放平台（控制台）](https://q.qq.com/)
- [QQ 机器人 API v2 文档](https://bot.q.qq.com/wiki/develop/api-v2/)
- [WebSocket 接入（Opcode、鉴权、Resume）](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/reference.html)
- [事件订阅与 intents](https://bot.q.qq.com/wiki/develop/api-v2/dev-prepare/interface-framework/event-emit.html)
- [获取 WSS 接入点](https://bot.q.qq.com/wiki/develop/api-v2/openapi/wss/url_get.html)
- [消息收发](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/send-receive/)
- [富媒体消息](https://bot.q.qq.com/wiki/develop/api-v2/server-inter/message/send-receive/rich-media.html)（实现 MCP `send_file` 时需参考）
