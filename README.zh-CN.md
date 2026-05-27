# cc-gateway

通过飞书 (Lark)、Telegram 机器人和 CLI 控制本地 Claude Code 的网关。

## 功能特性

- **远程控制**: 通过手机上的飞书 (Lark) 或 Telegram 机器人控制本地 Claude Code
- **聊天隔离**: 每个聊天都有独立的智能体子进程 — 不同群聊或用户的消息不会相互混淆
- **本地 CLI 聊天**: 交互式命令行聊天，支持 Tab 补全和行内提示
- **会话切换**: `/agent` 进入智能体会话模式；除网关内置命令外所有内容直接转发给当前智能体
- **目录选择器**: `/ll` 打开交互式目录选择器 (CLI 中为 TUI，飞书中为卡片)
- **守护进程模式**: 使用 `start/stop/restart/log` 命令作为后台服务运行，通过端口绑定保证单实例

## 安装

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.sh | sh
```

### Windows

```powershell
irm https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.ps1 | iex
```

### 从源码编译

```sh
git clone https://github.com/caixy-plus/cc-gateway.git
cd cc-gateway
cargo build --release
```

## 快速开始

1. **配置**

   ```sh
   cc-gateway config      # 在 $EDITOR 中打开配置
   # 或
   cc-gateway config --init > ~/.cc-gateway/config.json
   ```

   编辑 `~/.cc-gateway/config.json`:
   - 飞书: 将 `feishu.enabled` 设为 `true`，并设置 `feishu.app_id` 和 `feishu.app_secret` (从 [飞书开放平台](https://open.feishu.cn) 获取)
   - Telegram: 将 `telegram.enabled` 设为 `true`，并设置 `telegram.bot_token` (从 [@BotFather](https://t.me/BotFather) 获取)
   - 设置 `default_dir` 为远程用户应该浏览的目录 (例如 `~/Workspace`)
   - 两个平台可以同时启用

2. **启动守护进程**

   ```sh
   cc-gateway start
   ```

3. **从 CLI 聊天**

   ```sh
   cc-gateway
   cc-gateway> /agent
   💬 ~/Workspace ▶ hello, review this code for me
   ```

4. **停止守护进程**

   ```sh
   cc-gateway stop
   ```

## 命令

| 命令 | 说明 |
|---------|-------------|
| `cc-gateway` | 进入交互式 CLI 聊天模式 |
| `cc-gateway start` | 启动网关守护进程 |
| `cc-gateway stop` | 停止网关守护进程 |
| `cc-gateway restart` | 重启网关守护进程 |
| `cc-gateway log [-f] [-n 100]` | 查看守护进程日志 |
| `cc-gateway config` | 编辑配置文件 |
| `cc-gateway config --init` | 打印默认配置 |

## 网关命令 (聊天中可用)

| 命令 | 说明 |
|---------|-------------|
| `/help` | 显示可用命令 |
| `/quit` | 退出当前智能体会话 (未激活时 = 退出程序) |
| `/cd <path>` | 更改工作目录 |
| `/cd_default` | 将工作目录更改为默认目录 |
| `/agent [claude|cursor] [args...]` | 启动或重启智能体会话 (传递参数给对应 CLI) |
| `/agents [claude|cursor]` | 选择 / 设置本频道默认智能体 |
| `/agent-history [n]` | 显示最近会话并按索引恢复 |
| `/pwd` | 显示当前工作目录 |
| `/ll` | 打开交互式目录选择器 |
| `/mkdir <目录名>` | 创建新目录 |
| `/show-thinking` | 始终显示可用的 Thinking 输出 |
| `/hide-thinking` | 隐藏 Thinking 输出 |

### 会话切换

运行 `/agent` 后，网关进入 **会话模式**:
- 提示符变为 `💬 ~/Workspace ▶` 表示正在与智能体聊天
- 你输入的所有内容直接发送给当前智能体 (无需前缀)
- 输入 `/quit` 停止会话并返回网关命令模式

## 配置

完整配置说明请参阅 [docs/config.zh-CN.md](docs/config.zh-CN.md)。

## 架构

```
用户 (飞书/Lark)  <-->  cc-gateway 守护进程  <-->  Claude Code (本地)
用户 (Telegram)   <-->  cc-gateway 守护进程  <-->  Claude Code (本地)
用户 (CLI)        <-->  cc-gateway 守护进程  <-->  Claude Code (本地)
```

cc-gateway 通过 stdin/stdout 使用 `stream-json` 协议与 Claude Code 通信 (`--input-format stream-json --output-format stream-json`)。

## 许可证

MIT
