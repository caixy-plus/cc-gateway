# cc-gateway

**English** | [简体中文](README.zh-CN.md)

Gateway for controlling local agent CLIs (Claude Code, Codex, Cursor, Pi, OpenCode, Kimi, Gemini, Qoder, …) via **Feishu/Lark**, **Telegram**, **QQ**, and **WebUI**.

## Features

- **Remote control** — Use chat bots on your phone to drive agents running on your machine
- **Multi-platform** — Feishu/Lark, Telegram, and QQ official bots; enable any combination
- **Multi-provider** — Pluggable agent backends (`claude`, `codex`, `cursor`, `pi`, `opencode`, `kimi`, `gemini`, `qoder`, …); pick per chat with `/agents`
- **Per-chat isolation** — Each chat/channel gets its own agent subprocess; messages never mix across chats
- **Pairing** — Optional WebUI approval for new chats before they can use the bot (recommended)
- **WebUI** — Browser dashboard for sessions, pairing, settings, and live events
- **Directory tools** — `/ll` (Feishu card, Telegram inline keyboard, text list on QQ/WebUI)
- **Daemon mode** — `start` / `stop` / `restart` / `log`; single instance via port binding

## Installation

### macOS / Linux (release binary)

```sh
curl -fsSL https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/install.sh | sh
```

The installer runs `cc-gateway init`, restarts the daemon, and prints documentation links.

### Windows (release binary)

```powershell
irm https://raw.githubusercontent.com/caixy-plus/cc-gateway/main/scripts/install-irm.ps1 | iex
```

(Do not use `irm install.ps1 | iex`: that file has a UTF-8 BOM that can show as leading garbled text when piped; install still works.)

### From source (developers)

```sh
git clone https://github.com/caixy-plus/cc-gateway.git
cd cc-gateway
./install_local.sh    # macOS/Linux: builds WebUI + release binary to ~/.local/bin
# Windows: .\install_local.ps1
```

Or manually: `cargo build --release` (see [CLAUDE.md](CLAUDE.md) for WebUI embedding).

## Quick Start

1. **Initialize configuration**

   ```sh
   cc-gateway init
   ```

   Or edit `~/.cc-gateway/config.json` / use WebUI **Settings** after `cc-gateway start`. Legacy config layouts are upgraded and saved on first load ([config.md](docs/config.md)).

   Enable the bots you need, for example:

   | Platform | Config | Setup guide |
   |----------|--------|-------------|
   | Feishu / Lark | `platforms.feishu.enabled`, `app_id`, `app_secret` | [docs/bots/feishu.md](docs/bots/feishu.md) |
   | Telegram | `platforms.telegram.enabled`, `bot_token` | [docs/bots/telegram.md](docs/bots/telegram.md) |
   | QQ | `platforms.qq.enabled`, `app_id`, `app_secret`, `sandbox` | [docs/bots/qq.md](docs/bots/qq.md) |

   Set `default_dir` to the workspace root remote users should browse (e.g. `~/Workspace`). Multiple platforms can run at once.

2. **Start the daemon**

   ```sh
   cc-gateway start
   ```

3. **Open WebUI** (pairing, settings, sessions)

   ```sh
   cc-gateway webui
   ```

4. **Chat from WebUI or a connected bot** — use `/agent` in the WebUI message box or in Feishu/Telegram/QQ after pairing.

5. **Stop when done**

   ```sh
   cc-gateway stop
   ```

## CLI Commands

| Command | Description |
|---------|-------------|
| `cc-gateway` | Show help (same as `cc-gateway --help`) |
| `cc-gateway init` | Interactive setup wizard (config + optional bot credentials) |
| `cc-gateway start` | Start the gateway daemon |
| `cc-gateway stop` | Stop the gateway daemon |
| `cc-gateway restart` | Restart the gateway daemon |
| `cc-gateway status` | Show daemon status |
| `cc-gateway log [-f] [-n 100]` | View daemon logs (`-f` to follow) |
| `cc-gateway webui` | Open WebUI in the browser (starts daemon if needed) |
| `cc-gateway webui-token [--refresh]` | Show or regenerate WebUI access token |
| `cc-gateway enable` / `disable` | Toggle OS auto-start (launchd / systemd user unit) |
| `cc-gateway update [--check] [-f] [-y]` | Check or install latest release (`--check` = version only) |
| `cc-gateway uninstall [-y] [--keep-data]` | Remove binary and service entries (`--keep-data` keeps `~/.cc-gateway`) |

## Gateway Commands (in chat)

Available in WebUI and connected bots (after pairing if enabled):

| Command | Description |
|---------|-------------|
| `/help` | List gateway commands |
| `/quit` | Stop active agent session |
| `/cd <path>` | Change working directory |
| `/cd_default` | Reset working directory to `default_dir` |
| `/agent [provider] [args...]` | Start or restart agent session |
| `/agents [provider]` | Set this channel's default agent |
| `/agent-history [n]` | List recent sessions; resume by index |
| `/pwd` | Show current working directory |
| `/ll` | Pick directory (Feishu card / Telegram inline keyboard / text on QQ & WebUI) |
| `/mkdir <name>` | Create a directory |
| `/show-thinking` / `/hide-thinking` | Toggle Thinking output |
| `/stop` / `/clear` / `/status` / `/esc` | Control active generation (where supported) |

**Providers:** `claude`, `codex`, `cursor`, `pi`, `opencode`, `kimi`, `gemini`, `qoder` — run `/agents` to see enabled profiles.

### Session mode

After `/agent`, plain text is forwarded to the agent; gateway commands still work. `/quit` stops the active session.

## Documentation

| Topic | English | 中文 |
|-------|---------|------|
| Bot setup overview | [docs/bots/README.md](docs/bots/README.md) | [docs/bots/README.zh-CN.md](docs/bots/README.zh-CN.md) |
| Configuration | [docs/config.md](docs/config.md) | [docs/config.zh-CN.md](docs/config.zh-CN.md) |
| Usage (daemon & WebUI) | [docs/usage.md](docs/usage.md) | [docs/usage.zh-CN.md](docs/usage.zh-CN.md) |
| **Release (tag & CI)** | [docs/release.md](docs/release.md) | [docs/release.zh-CN.md](docs/release.zh-CN.md) |
| Developer guide | [CLAUDE.md](CLAUDE.md) | — |

Install scripts print these links again at the end of setup.

## Architecture

```
User (Feishu/Lark)  <--->  cc-gateway daemon  <--->  Agent CLIs (local)
User (Telegram)     <--->  cc-gateway daemon  <--->  claude / cursor / pi / …
User (QQ)           <--->  cc-gateway daemon
User (WebUI)        <--->  cc-gateway daemon
```

The gateway spawns provider CLIs as child processes and bridges chat traffic to them (e.g. Claude **stream-json** on stdio, Codex/Cursor/OpenCode/Kimi/Gemini/Qoder **ACP**, Pi **JSON-RPC**). See [CLAUDE.md](CLAUDE.md) for protocol details.

## License

MIT
