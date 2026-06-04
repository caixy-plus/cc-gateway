# 使用指南

## WebUI 与聊天机器人

通过守护进程 + WebUI 或已接入的平台（飞书、Telegram、QQ）与智能体对话。平台配置见 [bots/README.zh-CN.md](bots/README.zh-CN.md)。

```sh
cc-gateway init
cc-gateway start
cc-gateway webui
```

网关命令（`/agent`、`/cd`、`/ll` 等）在 WebUI 与各机器人聊天中可用。飞书 `/ll` 为交互卡片；Telegram、QQ、WebUI 为文本列表。

## 守护进程模式

守护进程通过绑定本地端口 (`port` 配置项，默认 `17534`) 来保证单实例运行。如果已有守护进程在运行，`start` 会报告现有 PID 而不会启动第二个进程。

### 启动

```sh
cc-gateway start
```

将 cc-gateway 作为后台守护进程启动。守护进程会同时监听所有已启用平台的消息（飞书、Telegram、QQ 可同时运行）。

### 停止

```sh
cc-gateway stop
```

优雅地关闭守护进程。所有活跃的聊天会话都会收到关闭通知，每个 Claude 子进程有 500 毫秒的时间退出，超时将被强制终止。

### 重启

```sh
cc-gateway restart
```

### 查看日志

```sh
cc-gateway log              # 显示最后 100 行
cc-gateway log -f           # 追踪日志输出
cc-gateway log -n 500       # 显示最后 500 行
```

## 新聊天配对

默认开启 `require_pairing` 时，用户首次给机器人发消息后，须在 **WebUI → 配对** 中批准（机器人会回复配对码），之后才能正常使用 `/agent`。详见 [bots/README.zh-CN.md](bots/README.zh-CN.md)。

## 飞书机器人

配置步骤：[bots/feishu.zh-CN.md](bots/feishu.zh-CN.md)。守护进程运行且飞书已配置后，你可以:

1. 打开飞书并找到你的机器人
2. 直接发送消息 — 当会话激活时它们会被转发给 Claude Code
3. 使用网关命令: `/cd`, `/agent`, `/agents`, `/agent-history`, `/pwd`, `/ll`, `/help`, `/quit`

每个聊天 (群聊或私聊) 都有独立的 Claude 子进程，不同聊天的消息不会相互混淆。

### 目录选择卡片

在飞书中发送 `/ll` 可收到一个交互式卡片，列出 `default_dir` 中的文件夹。点击文件夹按钮即可更改工作目录。

### 飞书中的命令边界

- `/cd ..` 只能导航到 `default_dir` — 尝试超出会返回访问被拒绝消息
- `/quit` 仅在 Claude 会话激活时有效；否则会收到提示消息

## Telegram 机器人

配置步骤：[bots/telegram.zh-CN.md](bots/telegram.zh-CN.md)。守护进程运行且 Telegram 已配置后:

1. 打开 Telegram 并找到你的机器人
2. 直接发送消息 — 它们会被转发给 Claude Code
3. 使用相同的网关命令

每个聊天都有独立的智能体子进程。Telegram 仅使用长轮询 (`getUpdates`)。

## QQ 机器人

配置步骤：[bots/qq.zh-CN.md](bots/qq.zh-CN.md)。守护进程运行且 QQ 已配置后:

1. **私聊（C2C）：** 直接与机器人对话。
2. **群聊：** 发送消息时需 **@ 机器人**。
3. 网关命令与 WebUI 相同（`/agent`、`/cd`、`/help` 等）。
4. `/ll`、选智能体等为 **纯文本**（无消息卡片）。

每个 QQ 频道（`u:…` / `g:…`）独立会话。修改 QQ 凭证或 `sandbox` 后需重启守护进程。

## 小贴士

- 使用 `/agent-history` 查看最近会话，再用 `/agent-history <n>` 按索引恢复
- 将敏感凭证保存在环境变量中，而非 config.json
- 如果默认端口被其他程序占用，可修改 `config.json` 中的 `port`，或让安装脚本自动检测空闲端口
