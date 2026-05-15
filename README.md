# cc-gateway

Gateway for controlling Claude Code via Feishu/Lark and CLI.

## Features

- **Remote Control**: Control your local Claude Code from your phone via Feishu (Lark) bot
- **Local CLI Chat**: Interactive command-line chat with the same capabilities as the Feishu bot
- **AI Intent Recognition** (optional): Automatically detect which local project you want to work on
- **Built-in Commands**: `/cd`, `/claude`, `/pwd`, `/model`, `/cc-quit`, `/cc/...`
- **Daemon Mode**: Run as a background service with `start/stop/restart/log` commands
- **Permission Handling**: Approve or deny Claude Code tool requests remotely

## Installation

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.sh | bash
```

### Windows

```powershell
irm https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.ps1 | iex
```

### From Source

```bash
git clone https://github.com/caixy-plus/cc-gateway.git
cd cc-gateway
cargo build --release
```

## Quick Start

1. **Configure**

   ```bash
   cc-gateway config      # Opens config in $EDITOR
   # or
   cc-gateway config --init > ~/.cc-gateway/config.json
   ```

   Edit `~/.cc-gateway/config.json`:
   - Set `feishu.app_id` and `feishu.app_secret` (from [Feishu Open Platform](https://open.feishu.cn))
   - Optionally set `ai.api_key` for smart project detection

2. **Start the daemon**

   ```bash
   cc-gateway start
   ```

3. **Chat from CLI**

   ```bash
   cc-gateway
   cc-gateway> /claude
   cc-gateway> hello, review this code for me
   ```

4. **Stop the daemon**

   ```bash
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
| `/cc-quit` | Quit current Claude session |
| `/cd <path>` | Change working directory and restart Claude |
| `/claude` | Start or restart Claude session |
| `/pwd` | Show current working directory |
| `/model <model>` | Switch Claude model |
| `/status` | Show gateway status |
| `/cc/<cmd>` | Forward slash command to Claude (e.g. `/cc/clear`) |

## Configuration

See [docs/config.md](docs/config.md) for full configuration reference.

## Architecture

```
User (Feishu/Lark)  <--->  cc-gateway daemon  <--->  Claude Code (local)
User (CLI)          <--->  cc-gateway daemon  <--->  Claude Code (local)
```

cc-gateway communicates with Claude Code via stdin/stdout using the `stream-json` protocol (`--input-format stream-json --output-format stream-json`).

## License

MIT
