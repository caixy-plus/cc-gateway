# 飞书 / Lark 机器人配置

通过 **WebSocket 长连接**（pbbp2）接入飞书自定义应用机器人。本机无需公网 Webhook 地址。

## 前置条件

- 可在飞书 / Lark 租户中创建并安装应用
- 已安装 cc-gateway（建议先执行 `cc-gateway init`）
- 服务器可访问飞书开放平台 HTTPS 接口

## 1. 在飞书开放平台创建应用

1. 打开 [飞书开放平台](https://open.feishu.cn/app)（国际版租户使用 [Lark](https://open.larksuite.com/app)）。
2. **创建企业自建应用** → 在「凭证与基础信息」记录 **App ID**、**App Secret**。
3. **应用能力** → 开启 **机器人**。
4. **事件与回调**：
   - 订阅 **`im.message.receive_v1`**（接收消息）。
   - 订阅方式选择 **WebSocket 长连接**（勿选仅 HTTP 回调，与 cc-gateway 实现一致）。
5. **权限管理**：按租户要求开通发消息、读消息等 IM 相关权限。
6. **版本管理与发布** → 创建版本并发布（若租户需管理员审核则先过审）。
7. **安装应用** 到目标企业 / 测试租户。

## 2. 配置 cc-gateway

编辑 `~/.cc-gateway/config.json`，或在 WebUI **设置 → 飞书** 填写：

```json
{
  "feishu": {
    "enabled": true,
    "app_id": "cli_xxxxxxxx",
    "app_secret": "你的AppSecret",
    "require_pairing": true
  },
  "default_dir": "/你的/项目/根目录"
}
```

| 字段 | 说明 |
|------|------|
| `enabled` | 守护进程启动时是否连接飞书 |
| `app_id` | 应用 App ID（`cli_...`） |
| `app_secret` | 应用密钥 |
| `require_pairing` | 新聊天是否须在 WebUI 配对放行 |

支持环境变量，例如 `"app_id": "${FEISHU_APP_ID}"`。

## 3. 启动与验证

```sh
cc-gateway start
cc-gateway log -f
```

日志中应出现飞书 WebSocket 连接信息。WebUI **平台** 页在连接正常时显示飞书在线。

## 4. 配对放行（`require_pairing: true` 时）

1. 在飞书与机器人 **私聊** 或 **群聊** 中发送任意消息。
2. 机器人回复 **配对码**。
3. 打开 WebUI → **配对** → 批准对应飞书会话（平台为 `feishu`）。
4. 发送 `/agent` 启动智能体后即可对话。

## 5. 使用说明

- **`/ll`**：发送 **交互卡片**，列出 `default_dir` 下目录；点击按钮切换工作目录。
- **`/cd`**：飞书模式下路径不能超出 `default_dir` 上级。
- **会话隔离**：每个群聊 / 私聊独立子进程与频道会话。
- **卡片**：仅飞书支持目录卡片；其他平台 `/ll` 为纯文本列表。

## 故障排查

| 现象 | 排查项 |
|------|--------|
| 收不到消息 | 是否订阅 `im.message.receive_v1`；是否为 WebSocket 模式；应用是否已安装 |
| 鉴权失败 | `app_id` / `app_secret`；应用是否已发布 |
| 发消息无响应 | 是否已完成配对；WebUI 配对队列 |
| 群内不可用 | 机器人是否入群；权限与 @ 规则 |

## 参考

- [飞书开放平台文档](https://open.feishu.cn/document/home/index)
- [创建应用（控制台）](https://open.feishu.cn/app)
- [Lark 国际版控制台](https://open.larksuite.com/app)
- [机器人 WebSocket 长连接](https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/server-side-sdk/golang-sdk-guide/preparations)
- [卡片 JSON v2](https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes)
- [按钮组件（V2）](https://open.feishu.cn/document/feishu-cards/card-json-v2-components/interactive-components/button)
