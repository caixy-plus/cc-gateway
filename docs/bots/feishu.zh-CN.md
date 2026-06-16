# 飞书 / Lark 机器人配置

通过 **WebSocket 长连接**（pbbp2）接入飞书自定义应用机器人。cc-gateway 会主动连接飞书开放平台，本机无需公网 IP、域名或 HTTP Webhook 回调地址。

本文按飞书开放平台当前的消息事件、WebSocket 事件订阅、IM 消息 API 文档整理。飞书国内版与 Lark 国际版的控制台文案可能略有差异，但需要创建的应用形态一致。

## 前置条件

- 可在飞书 / Lark 租户中创建 **企业自建应用 / Custom App** 并安装到企业
- 本机已安装 cc-gateway
- 至少配置了一个可用智能体 provider，通常通过 `cc-gateway init` 完成
- 运行 cc-gateway 的机器可访问飞书 / Lark 开放平台接口

## 1. 安装并初始化 cc-gateway

先初始化网关，确保配置目录与 WebUI 可用：

```sh
cc-gateway init
cc-gateway webui
```

`cc-gateway webui` 会在需要时启动守护进程并打开本地 WebUI。后续配对放行会用到 WebUI。

## 2. 创建飞书 / Lark 应用

1. 打开 [飞书开放平台](https://open.feishu.cn/app)（国际版租户使用 [Lark Developer Console](https://open.larksuite.com/app)）。
2. 创建 **企业自建应用 / Custom App**，不要创建小程序、H5 应用或仅 Webhook 机器人。
3. 进入应用的 **凭证与基础信息**，复制：
   - **App ID**（`cli_...`）
   - **App Secret**
4. 添加或启用 **机器人** 能力。
5. 配置机器人的 **可用范围 / 可见范围**，确保目标用户或群组能添加并使用机器人。

## 3. 配置应用权限

进入飞书控制台 **权限管理**，搜索并添加下列权限（可按权限名称或权限标识搜索），然后保存。

飞书也支持 **批量导入/导出权限**。cc-gateway 普通聊天的最小必需权限可以直接导入：进入 **权限管理 → 批量导入/导出权限 → 导入**，用下面内容替换示例 JSON：

```json
{
  "scopes": {
    "tenant": [
      "im:message:send_as_bot",
      "im:message.p2p_msg:readonly",
      "im:message.group_at_msg:readonly"
    ]
  }
}
```

这份 JSON 刻意保持最小：允许机器人接收单聊消息、接收群聊 @ 消息，并发送回复 / 交互卡片。如果你的租户控制台提示某个权限已被新版权限替代，以飞书当前展示的替代权限为准。

普通 cc-gateway 聊天的最小权限：

| 用途 | 权限 / Scope |
|------|--------------|
| 以机器人身份发送回复、富文本、交互卡片 | `im:message:send_as_bot` 或更宽的 `im:message` |
| 接收用户发给机器人的单聊消息 | `im:message.p2p_msg:readonly` |
| 接收群聊中 @ 机器人的消息 | `im:message.group_at_msg:readonly` |

使用附件或 agent `send_file` 时建议额外开通：

| 用途 | 权限 / Scope |
|------|--------------|
| 上传 / 下载消息图片和文件资源 | `im:resource` 或控制台当前的“获取/上传 IM 资源”权限 |
| 群聊中不 @ 机器人也接收所有消息 | `im:message.group_msg:readonly`（敏感权限，仅在租户允许且确有需要时申请） |

注意：

- 飞书已下线部分旧版消息权限。如果控制台同时出现旧权限与新权限，优先选择上表中的 `:readonly` 或更宽的替代权限。
- cc-gateway 重度依赖飞书交互卡片。`/ll`、`/agents`、`/models`、`/agent-history`、权限 Allow / Deny、选择器和确认提示都使用卡片。发送卡片依赖上面的消息发送权限；点击按钮的回传需要在下一步事件 / 回调订阅中配置。
- 权限批量导入只导入 API 权限，不会自动订阅事件或卡片回调；下一步仍需单独配置 `im.message.receive_v1` 和 `card.action.trigger`。
- 权限变更不会自动对已安装用户生效，必须创建并发布新版本，必要时等待管理员审批并重新安装。

## 4. 配置事件订阅

进入 **事件与回调 / 事件订阅**：

1. 开启事件订阅。
2. 订阅方式选择 **WebSocket / 长连接**。cc-gateway 不使用 HTTP 回调地址。
3. 添加事件 **`im.message.receive_v1`**（接收消息）。
4. 添加卡片交互回调 **`card.action.trigger`**（卡片动作触发 / 卡片回调）。如果控制台把“事件订阅”和“卡片回传交互”分开配置，请进入卡片回调配置页，开启新版卡片回调流程。
5. 确认卡片回调也通过 **WebSocket / 长连接** 接收。不要为卡片回调切换到 HTTP 回调 URL；cc-gateway 不提供 HTTP 回调服务。
6. 保存事件配置。

cc-gateway 会用 App ID / App Secret 向飞书获取 WebSocket 连接地址。你不需要在 cc-gateway 里配置 Verification Token、Encrypt Key 或回调 URL。

为什么必须配置：飞书把普通用户消息作为 `im.message.receive_v1` 推送，但交互卡片的按钮点击会单独作为 `card.action.trigger` 回调推送。cc-gateway 的 `/ll` 目录翻页与切换、`/agents`、`/models`、`/agent-history`、权限 **Allow / Deny** 按钮和其他卡片选择都依赖这个回调。如果漏配 `card.action.trigger`，常见现象是文字对话正常，但卡片按钮点击后没有任何响应。

## 5. 发布并安装应用

每次修改机器人能力、权限或事件订阅后：

1. 进入 **版本管理与发布**。
2. 创建新版本。
3. 如果租户要求管理员审核，提交审核并等待通过。
4. 将应用安装 / 重新安装到目标企业。
5. 将机器人添加到目标群聊，或打开与机器人的单聊。

如果跳过发布安装，常见现象是 WebSocket 已连接但完全收不到消息事件。

## 6. 配置 cc-gateway

编辑 `~/.cc-gateway/config.json`，或在 WebUI **设置 → 飞书** 填写：

```json
{
  "platforms": {
    "feishu": {
      "enabled": true,
      "app_id": "cli_xxxxxxxx",
      "app_secret": "你的AppSecret",
      "require_pairing": true
    }
  },
  "default_dir": "/你的/项目/根目录"
}
```

（此处省略 `agent`、`log`、`port` 等顶层字段；完整结构见 [config.zh-CN.md](../config.zh-CN.md)。）

| 字段 | 说明 |
|------|------|
| `enabled` | 守护进程启动时是否连接飞书 |
| `app_id` | 应用 App ID（`cli_...`） |
| `app_secret` | 应用密钥 |
| `require_pairing` | 新聊天是否须在 WebUI 配对放行 |

支持环境变量，例如 `"app_id": "${FEISHU_APP_ID}"`。

修改 `enabled`、`app_id`、`app_secret` 后需要重启守护进程；在 WebUI 修改 `require_pairing` 可即时生效。

## 7. 启动与验证

```sh
cc-gateway restart
cc-gateway status
cc-gateway log -f
```

日志中应看到类似信息：

- `Starting Feishu platform...`
- `Feishu WebSocket endpoint: ...`
- `Feishu WebSocket connected successfully`

WebUI **平台** 页在连接正常时会显示飞书在线。

然后在飞书发送测试消息：

- 单聊：直接给机器人发送 `hello` 或 `/help`。
- 群聊：先把机器人添加到群里，再发送 `@机器人 /help`。除非你申请了群聊全量消息权限，否则群聊通常需要 @ 机器人。

## 8. 配对放行（`require_pairing: true` 时）

1. 在飞书与机器人 **私聊** 或 **群聊** 中发送任意消息。
2. 机器人回复 **配对码**。
3. 打开 WebUI → **配对** → 批准对应飞书会话（平台为 `feishu`）。
4. 发送 `/agent` 启动智能体后即可对话。

如果机器人只回复配对码，说明开放平台配置已基本打通；先在 WebUI 放行该聊天，再测试智能体命令。

## 9. 支持的消息行为

入站消息：

- 文本与富文本会转成用户文本发送给智能体。
- 图片、文件、音频、视频 / 媒体、富文本中的图片会下载到 `~/.cc-gateway/media/`，再以本地文件引用形式转发给智能体。
- 空消息或暂不支持的消息类型会在 ACK 飞书事件后忽略。

出站消息：

- 普通 assistant 输出使用文本 / 富文本发送。
- `/ll`、`/agents`、`/models`、`/agent-history`、权限确认、选择器和确认提示等使用飞书交互卡片。
- 卡片按钮点击必须通过 WebSocket / 长连接接收 `card.action.trigger` 回调。飞书返回卡片上下文后，cc-gateway 会在需要时原地更新卡片状态。
- MCP `send_file` 会把图片发送为飞书图片消息，其他文件发送为飞书文件消息。

- **`/ll`**：发送 **交互卡片**，列出**当前**工作目录子文件夹（初始为 `default_dir`）；点击按钮 `cd`。
- **`/cd`**：与其他平台相同，路径须落在用户主目录内（`ensure_under_home`），不限于 `default_dir`。
- **会话隔离**：每个群聊 / 私聊独立子进程与频道会话。
- **MCP `send_file` 限制**：飞书图片上传最大 **10 MB**，文件上传最大 **30 MB**。

## 故障排查

| 现象 | 排查项 |
|------|--------|
| WebUI 显示飞书未连接 | `app_id` / `app_secret`；机器能否访问飞书开放平台；日志中 `callback/ws/endpoint` 附近错误 |
| WebSocket 已连接但收不到消息 | 是否添加 `im.message.receive_v1`；订阅方式是否为 **WebSocket 长连接**；权限是否发布生效；应用是否安装到企业 |
| 单聊不触发 | 机器人能力是否开启；应用可见范围是否包含该用户；`im:message.p2p_msg:readonly` 或替代权限是否已审批 |
| 群聊不触发 | 机器人是否入群；是否 @ 机器人；`im:message.group_at_msg:readonly` 是否已审批；如希望免 @，需申请 `im:message.group_msg:readonly` |
| 能收到消息但无法回复 | `im:message:send_as_bot` 或 `im:message`；机器人是否仍在群内；群是否允许机器人发言 |
| 卡片消息发不出去 | `im:message:send_as_bot` 或 `im:message`；应用是否重新发布；查看日志中 `send_interactive_card` 附近错误 |
| 卡片已发送但按钮点击无响应 | 是否添加 `card.action.trigger`；是否开启卡片回传交互；卡片回调是否使用 **WebSocket / 长连接** 而不是 HTTP；应用是否重新发布；日志中是否出现 `card.action.trigger` |
| `/ll`、`/agents`、`/models` 或权限按钮无响应 | 同“卡片按钮点击无响应”；这些功能都依赖 `card.action.trigger` |
| 附件失败 | IM 资源上传 / 下载权限；文件大小是否超过飞书限制；查看 `~/.cc-gateway/logs/gateway.log` |
| 机器人只回复配对码 | 在 WebUI → 配对 中批准该聊天；私有测试机器人也可设 `require_pairing: false` |
| 修改配置后无变化 | 改 `enabled`、`app_id`、`app_secret` 后需重启；飞书控制台改权限/事件后需重新发布并安装 |

## 参考

- [飞书开放平台文档](https://open.feishu.cn/document/home/index)
- [创建应用（控制台）](https://open.feishu.cn/app)
- [Lark 国际版控制台](https://open.larksuite.com/app)
- [接收消息事件（`im.message.receive_v1`）](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive?lang=zh-CN)
- [发送消息 API](https://open.feishu.cn/document/server-docs/im-v1/message/create?lang=zh-CN)
- [通过 WebSocket 接收事件](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case?lang=zh-CN)
- [配置卡片交互](https://open.feishu.cn/document/feishu-cards/configuring-card-interactions?lang=zh-CN)
- [处理卡片回调](https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/handle-card-callbacks?lang=zh-CN)
- [卡片回调通信（`card.action.trigger`）](https://open.feishu.cn/document/feishu-cards/card-callback-communication?lang=zh-CN)
- [通过长连接接收回调](https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address?lang=zh-CN)
- [卡片 JSON v2](https://open.feishu.cn/document/uAjLw4CM/ukzMukzMukzM/feishu-cards/card-json-v2-breaking-changes-release-notes)
- [按钮组件（V2）](https://open.feishu.cn/document/feishu-cards/card-json-v2-components/interactive-components/button)
- [上传图片](https://open.feishu.cn/document/server-docs/im-v1/image/create?lang=zh-CN)
- [上传文件](https://open.feishu.cn/document/server-docs/im-v1/file/create?lang=zh-CN)
