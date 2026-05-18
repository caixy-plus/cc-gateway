# 使用指南

## 交互模式

不携带任何子命令运行 `cc-gateway` 即可进入交互式聊天模式:

```sh
$ cc-gateway
cc-gateway 交互模式  输入 '/help' 查看命令，'/quit' 退出。

cc-gateway> /claude
Claude 会话已启动于: /Users/you/Workspace

💬 ~/Workspace ▶ hello Claude
Hello! How can I help you today?

💬 ~/Workspace ▶ /quit
Claude 会话已停止。

cc-gateway> /quit
```

### 命令补全

输入 `/` 后按 `Tab` 查看可用命令列表及行内描述。

### 会话切换

- `/claude` — 进入 Claude 会话模式。提示符变为 `💬 ~/Workspace ▶`
- 在会话模式下，你输入的所有内容直接发送给 Claude
- `/quit` — 停止会话并返回网关模式
- 未进入会话时，`/quit` 直接退出程序

### 目录导航

```sh
cc-gateway> /cd ~/Projects/my-app
工作目录已更改为: /Users/you/Projects/my-app

cc-gateway> /ll
# 打开交互式 TUI 目录选择器
# 使用 ↑↓ 导航, Enter 确认, q 取消
```

## 守护进程模式

### 启动

```sh
cc-gateway start
```

将 cc-gateway 作为后台守护进程启动。守护进程会监听飞书消息 (如果已配置)。

### 停止

```sh
cc-gateway stop
```

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

## 飞书机器人

守护进程运行且飞书已配置后，你可以:

1. 打开飞书并找到你的机器人
2. 直接发送消息 — 当会话激活时它们会被转发给 Claude Code
3. 使用与 CLI 模式相同的网关命令: `/cd`, `/claude`, `/pwd`, `/ll`, `/help`, `/quit`

### 目录选择卡片

在飞书中发送 `/ll` 可收到一个交互式卡片，列出 `default_dir` 中的文件夹。点击文件夹按钮即可更改工作目录。

### 飞书中的命令边界

- `/cd ..` 只能导航到 `feishu.default_dir` — 尝试超出会返回访问被拒绝消息
- `/quit` 仅在 Claude 会话激活时有效；否则会收到提示消息

## 小贴士

- 使用 `/claude --resume <id>` 恢复之前的 Claude 会话
- 将敏感凭证保存在环境变量中，而非 config.json
