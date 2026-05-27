# cc-gateway

Gateway for controlling Claude Code via Feishu/Lark, Telegram, and CLI.

## Features

- **Remote Control**: Control your local Claude Code from your phone via Feishu (Lark) or Telegram bot
- **Per-Chat Isolation**: Each chat gets its own agent subprocess — messages from different groups or users never mix
- **Local CLI Chat**: Interactive command-line chat with tab completion and inline hints
- **Session Switching**: `/agent` enters agent session mode; everything except gateway builtins is forwarded to the active agent
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
   cc-gateway> /agent
   💬 ~/Workspace ▶ hello, review this code for me
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
| `/quit` | Quit current agent session (inactive = exit program) |
| `/cd <path>` | Change working directory |
| `/cd_default` | Change working directory to default |
| `/agent [claude|cursor] [args...]` | Start or restart an agent session (pass args to the configured CLI) |
| `/agents [claude|cursor]` | Pick / set this channel's default agent |
| `/agent-history [n]` | Show recent sessions and resume by index |
| `/pwd` | Show current working directory |
| `/ll` | Open interactive directory picker |
| `/mkdir <dirname>` | Create a directory |
| `/show-thinking` | Always show Thinking output when available |
| `/hide-thinking` | Hide Thinking output |

### Session Switching

After running `/agent`, the gateway enters **session mode**:
- The prompt changes to `💬 ~/Workspace ▶` to indicate you are chatting with the agent
- Everything you type is sent directly to the active agent (no prefix needed)
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
