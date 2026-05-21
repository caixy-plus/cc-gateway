# cc-gateway

Gateway for controlling Claude Code via Feishu/Lark, Telegram, and CLI.

## Features

- **Remote Control**: Control your local Claude Code from your phone via Feishu (Lark) or Telegram bot
- **Per-Chat Isolation**: Each chat gets its own Claude subprocess — messages from different groups or users never mix
- **Local CLI Chat**: Interactive command-line chat with tab completion and inline hints
- **Session Switching**: `/claude` enters Claude session mode; everything except `/quit` is forwarded directly to Claude
- **Directory Picker**: `/ll` opens an interactive directory picker (TUI in CLI, card in Feishu)
- **Daemon Mode**: Run as a background service with `start/stop/restart/log` commands. Single-instance enforced via port binding.

## Installation

### macOS / Linux

```sh
curl -fsSL https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.sh | sh
```

### Windows

```powershell
irm https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.ps1 | iex
```

### From Source

```sh
git clone https://github.com/caixy-plus/cc-gateway.git
cd cc-gateway
cargo build --release
```

## Quick Start

1. **Configure**

   ```sh
   cc-gateway config      # Opens config in $EDITOR
   # or
   cc-gateway config --init > ~/.cc-gateway/config.json
   ```

   Edit `~/.cc-gateway/config.json`:
   - For Feishu: set `feishu.enabled` to `true`, and set `feishu.app_id` and `feishu.app_secret` (from [Feishu Open Platform](https://open.feishu.cn))
   - For Telegram: set `telegram.enabled` to `true`, and set `telegram.bot_token` (from [@BotFather](https://t.me/BotFather))
   - Set `default_dir` to the directory remote users should browse (e.g. `~/Workspace`)
   - Both platforms can be enabled at the same time

2. **Start the daemon**

   ```sh
   cc-gateway start
   ```

3. **Chat from CLI**

   ```sh
   cc-gateway
   cc-gateway> /claude
   ǔcw Working directory ▶ hello, review this code for me
   ```

4. **Stop the daemon**

   ```sh
   cc-gateway stop
   ```

## Commands

| Command | Description |
|---------|-------------|
| `cc-gateway` | Enter interactive CLI chat mode |
| `cc-gateway start` | Start the gateway daemon |
| `cc-gateway stop` | Stop the gateway daemon |
| `cc-gateway restart` | Restart the gateway daemon |
| `cc-gateway log [-f] [-n 100]` | View daemon logs |
| `cc-gateway config` | Edit configuration file |
| `cc-gateway config --init` | Print default config |

## Gateway Commands (available in chat)

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/quit` | Quit current Claude session (inactive = exit program) |
| `/cd <path>` | Change working directory and restart Claude |
| `/claude [args...]` | Start or restart Claude session (pass args to Claude CLI) |
| `/pwd` | Show current working directory |
| `/ll` | Open interactive directory picker |

### Session Switching

After running `/claude`, the gateway enters **session mode**:
- The prompt changes to `ǔcw ~/Workspace ▶` to indicate you are chatting with Claude
- Everything you type is sent directly to Claude (no prefix needed)
- Type `/quit` to stop the session and return to gateway command mode

## Configuration

See [docs/config.md](docs/config.md) for full configuration reference.

## Architecture

```
User (Feishu/Lark)  <--->  cc-gateway daemon  <--->  Claude Code (local)
User (Telegram)     <--->  cc-gateway daemon  <--->  Claude Code (local)
User (CLI)          <--->  cc-gateway daemon  <--->  Claude Code (local)
```

cc-gateway communicates with Claude Code via stdin/stdout using the `stream-json` protocol (`--input-format stream-json --output-format stream-json`).

## License

MIT
