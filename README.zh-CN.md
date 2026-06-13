# cc-gateway

[English](README.md) | **简体中文**

通过 **飞书/Lark**、**Telegram**、**QQ** 与 **WebUI**，在本地运行并远程驱动多种智能体 CLI（Claude Code、Codex、Cursor、Pi、OpenCode、Kimi、Gemini、Qoder 等）。

## 功能特性

- **远程控制** — 用手机上的聊天机器人操作本机智能体
- **多平台** — 飞书/Lark、Telegram、QQ 官方机器人，可同时启用
- **多智能体** — 可插拔后端（`claude`、`codex`、`cursor`、`pi`、`opencode`、`kimi`、`gemini`、`qoder` 等），用 `/agents` 按聊天指定默认智能体
- **聊天隔离** — 每个聊天/频道独立子进程，消息互不串线
- **配对放行** — 可在 WebUI 中批准新聊天后再允许使用（建议开启）
- **WebUI** — 浏览器管理会话、配对、设置与实时事件
- **目录工具** — `/ll`（飞书交互卡片、Telegram 内联键盘、QQ/WebUI 文本列表）
- **守护进程** — `start` / `stop` / `restart` / `log`，端口绑定保证单实例

## 安装

### macOS / Linux（发布版二进制）

```sh
curl -fsSL https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.sh | sh
```

安装过程会执行 `cc-gateway init`、重启守护进程，并输出文档链接。

### Windows（发布版二进制）

```powershell
irm https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/scripts/install-irm.ps1 | iex
```

（勿使用 `irm install.ps1 | iex`：该文件含 UTF-8 BOM，管道执行时控制台可能出现首行乱码；安装逻辑不受影响。）

### 从源码（开发者）

```sh
git clone https://github.com/caixy-plus/cc-gateway.git
cd cc-gateway
./install_local.sh    # macOS/Linux：构建 WebUI 并安装到 ~/.local/bin
# Windows: .\install_local.ps1
```

或手动 `cargo build --release`（WebUI 嵌入说明见 [CLAUDE.md](CLAUDE.md)）。

## 快速开始

1. **初始化配置**

   ```sh
   cc-gateway init
   ```

   也可在 `cc-gateway start` 后通过 WebUI **设置** 或编辑 `~/.cc-gateway/config.json`。旧版配置会在首次加载时自动迁移并写回（见 [config.zh-CN.md](docs/config.zh-CN.md)）。

   按需启用机器人，例如：

   | 平台 | 配置项 | 配置指南 |
   |------|--------|----------|
   | 飞书 / Lark | `platforms.feishu.enabled`、`app_id`、`app_secret` | [docs/bots/feishu.zh-CN.md](docs/bots/feishu.zh-CN.md) |
   | Telegram | `platforms.telegram.enabled`、`bot_token` | [docs/bots/telegram.zh-CN.md](docs/bots/telegram.zh-CN.md) |
   | QQ | `platforms.qq.enabled`、`app_id`、`app_secret`、`sandbox` | [docs/bots/qq.zh-CN.md](docs/bots/qq.zh-CN.md) |

   将 `default_dir` 设为远程用户可浏览的工作区根目录（如 `~/Workspace`）。多个平台可同时运行。

2. **启动守护进程**

   ```sh
   cc-gateway start
   ```

3. **打开 WebUI**（配对、设置、会话）

   ```sh
   cc-gateway webui
   ```

4. **在 WebUI 或已接入的机器人中对话** — 配对后在 WebUI 输入框或飞书/Telegram/QQ 中使用 `/agent`。

5. **停止服务**

   ```sh
   cc-gateway stop
   ```

## CLI 命令

| 命令 | 说明 |
|------|------|
| `cc-gateway` | 显示帮助（同 `cc-gateway --help`） |
| `cc-gateway init` | 交互式配置向导（含可选机器人凭证） |
| `cc-gateway start` | 启动网关守护进程 |
| `cc-gateway stop` | 停止网关守护进程 |
| `cc-gateway restart` | 重启网关守护进程 |
| `cc-gateway status` | 查看守护进程状态 |
| `cc-gateway log [-f] [-n 100]` | 查看日志（`-f` 跟踪） |
| `cc-gateway webui` | 在浏览器打开 WebUI（未运行时会先 start） |
| `cc-gateway webui-token [--refresh]` | 查看或重新生成 WebUI 访问令牌 |
| `cc-gateway enable` / `disable` | 开关开机自启（launchd / systemd 用户服务） |
| `cc-gateway update [--check] [-f] [-y]` | 检查或安装最新发布版（`--check` 仅查版本） |
| `cc-gateway uninstall [-y] [--keep-data]` | 卸载二进制与服务项（`--keep-data` 保留 `~/.cc-gateway`） |

## 网关命令（聊天内）

在 WebUI 及已接入的机器人中可用（若开启配对须先放行）：

| 命令 | 说明 |
|------|------|
| `/help` | 显示命令列表 |
| `/quit` | 结束当前智能体会话 |
| `/cd <路径>` | 更改工作目录 |
| `/cd_default` | 恢复为 `default_dir` |
| `/agent [provider] [参数...]` | 启动或重启智能体会话 |
| `/agents [provider]` | 设置本频道默认智能体 |
| `/agent-history [n]` | 列出最近会话；按序号恢复 |
| `/pwd` | 显示当前工作目录 |
| `/ll` | 选择目录（飞书卡片 / Telegram 内联键盘 / QQ·WebUI 文本） |
| `/mkdir <名称>` | 创建目录 |
| `/show-thinking` / `/hide-thinking` | 开关 Thinking 输出 |
| `/stop` / `/clear` / `/status` / `/esc` | 控制当前生成（视平台支持） |

**智能体：** `claude`、`codex`、`cursor`、`pi`、`opencode`、`kimi`、`gemini`、`qoder` — 运行 `/agents` 查看已启用的配置。

### 会话模式

执行 `/agent` 后，普通文本会转发给智能体；网关命令仍可使用。`/quit` 结束当前会话。

## 文档

| 主题 | 中文 | English |
|------|------|---------|
| 机器人配置总览 | [docs/bots/README.zh-CN.md](docs/bots/README.zh-CN.md) | [docs/bots/README.md](docs/bots/README.md) |
| 配置说明 | [docs/config.zh-CN.md](docs/config.zh-CN.md) | [docs/config.md](docs/config.md) |
| 使用指南 | [docs/usage.zh-CN.md](docs/usage.zh-CN.md) | [docs/usage.md](docs/usage.md) |
| **发版（tag 与 CI）** | [docs/release.zh-CN.md](docs/release.zh-CN.md) | [docs/release.md](docs/release.md) |
| 开发者说明 | [CLAUDE.md](CLAUDE.md) | — |

安装脚本结束时会再次打印上述链接。

## 架构

```
用户 (飞书/Lark)  <-->  cc-gateway 守护进程  <-->  本地智能体 CLI
用户 (Telegram)   <-->  cc-gateway 守护进程  <-->  claude / cursor / pi / …
用户 (QQ)         <-->  cc-gateway 守护进程
用户 (WebUI)      <-->  cc-gateway 守护进程
```

网关以子进程方式拉起各 provider CLI，并桥接聊天消息（例如 Claude 的 **stream-json**、Codex/Cursor/OpenCode/Kimi/Gemini/Qoder 的 **ACP**、Pi 的 **JSON-RPC**）。协议细节见 [CLAUDE.md](CLAUDE.md)。

## 许可证

MIT
