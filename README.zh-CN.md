# cc-gateway

通过飞书 (Lark) 机器人和 CLI 控制本地 Claude Code 的网关。

## 功能特性

- **远程控制**: 通过手机上的飞书 (Lark) 机器人控制本地 Claude Code
- **本地 CLI 聊天**: 交互式命令行聊天，支持 Tab 补全和行内提示
- **会话切换**: `/claude` 进入 Claude 会话模式；除 `/quit` 外所有内容直接转发给 Claude
- **目录选择器**: `/ll` 打开交互式目录选择器 (CLI 中为 TUI，飞书中为卡片)
- **守护进程模式**: 使用 `start/stop/restart/log` 命令作为后台服务运行

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
   - 设置 `feishu.app_id` 和 `feishu.app_secret` (从 [飞书开放平台](https://open.feishu.cn) 获取)
   - 设置 `feishu.default_dir` 为飞书用户应该浏览的目录 (例如 `~/Workspace`)

2. **启动守护进程**

   ```sh
   cc-gateway start
   ```

3. **从 CLI 聊天**

   ```sh
   cc-gateway
   cc-gateway> /claude
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
| `/quit` | 退出当前 Claude 会话 (未激活时 = 退出程序) |
| `/cd <path>` | 更改工作目录并重启 Claude |
| `/claude [args...]` | 启动或重启 Claude 会话 (传递参数给 Claude CLI) |
| `/pwd` | 显示当前工作目录 |
| `/ll` | 打开交互式目录选择器 |

### 会话切换

运行 `/claude` 后，网关进入 **会话模式**:
- 提示符变为 `💬 ~/Workspace ▶` 表示正在与 Claude 聊天
- 你输入的所有内容直接发送给 Claude (无需前缀)
- 输入 `/quit` 停止会话并返回网关命令模式

## 配置

完整配置说明请参阅 [docs/config.zh-CN.md](docs/config.zh-CN.md)。

## 架构

```
用户 (飞书/Lark)  <--->  cc-gateway 守护进程  <--->  Claude Code (本地)
用户 (CLI)          <--->  cc-gateway 守护进程  <--->  Claude Code (本地)
```

cc-gateway 通过 stdin/stdout 使用 `stream-json` 协议与 Claude Code 通信 (`--input-format stream-json --output-format stream-json`)。

## 许可证

MIT
