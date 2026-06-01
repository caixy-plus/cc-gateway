# QQ 官方机器人配置

接入 **QQ 开放平台** 官方机器人（OpenAPI v2），通过 **WebSocket Gateway** 长连接收消息。适合在本机运行守护进程，无需公网 Webhook。

> 官方文档在大规模生产环境更推荐 Webhook；cc-gateway 当前实现为 **WebSocket Gateway**，便于自建部署。

## 前置条件

- QQ 开放平台开发者账号及已创建的 **机器人** 应用
- 控制台中的 **AppID**、**AppSecret**（客户端密钥）
- cc-gateway 可访问 QQ API 域名
- 机器人已开通 **C2C** 与 **群 @ 消息** 相关能力与事件（cc-gateway 使用 `GROUP_AND_C2C` 意图）

## 1. 在 QQ 开放平台创建机器人

1. 打开 [QQ 开放平台](https://q.qq.com/)。
2. 创建 **机器人** 应用并完成审核（按平台要求）。
3. 在应用详情复制 **AppID**、**AppSecret**。
4. 开通所需消息能力：
   - **C2C 私聊** — 事件 `C2C_MESSAGE_CREATE`
   - **群聊 @ 机器人** — 事件 `GROUP_AT_MESSAGE_CREATE`
5. 测试阶段可在配置中设 `"sandbox": true` 使用沙箱 API；正式上线改为 `false`。

## 2. 配置 cc-gateway

编辑 `~/.cc-gateway/config.json` 或 WebUI **设置 → QQ**：

```json
{
  "qq": {
    "enabled": true,
    "app_id": "102xxxxxx",
    "app_secret": "你的AppSecret",
    "sandbox": false,
    "require_pairing": true
  }
}
```

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

内部频道 ID 规则：

| 场景 | 频道 ID | 使用方式 |
|------|---------|----------|
| C2C 私聊 | `u:{user_openid}` | 直接与机器人私聊 |
| 群聊 | `g:{group_openid}` | 群内 **@ 机器人** 发消息 |

1. 发送任意消息（群聊须 @ 机器人）。
2. 机器人回复配对码。
3. WebUI → **配对** → 批准平台 `qq`。
4. 发送 `/agent` 后开始对话。

## 5. 使用说明

- **`/ll`、`/agents`**：纯文本列表（暂无 QQ 消息卡片）。
- **权限确认**：文本提示为主（无 Telegram 式内联按钮）。
- **MCP `send_file`**：通过富媒体（`msg_type` 7）支持。**私聊**：图片、视频、语音及一般文件。**群聊**：仅图片/视频/语音，PDF 等请私聊发送。流程为先 `POST …/files` 上传再带 `media.file_info` 发送。
- **修改** `app_id`、`app_secret`、`sandbox`、`enabled` 后需 **重启守护进程**。

## 故障排查

| 现象 | 排查项 |
|------|--------|
| Token / Gateway 失败 | AppID、AppSecret；沙箱与正式环境是否一致 |
| 无私聊消息 | 控制台是否开通 C2C 与对应事件 |
| 群无消息 | 机器人是否在群内；是否 @ 机器人；群事件权限 |
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
