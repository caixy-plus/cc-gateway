# Usage Guide

## WebUI and chat bots

Use the daemon plus WebUI or a connected platform (Feishu, Telegram, QQ) to talk to agents. See [docs/bots/README.md](bots/README.md) for platform setup.

```sh
cc-gateway init
cc-gateway start
cc-gateway webui
```

Gateway commands (`/agent`, `/cd`, `/ll`, …) work in WebUI and in bot chats. `/ll` is an interactive card on Feishu and a text list on Telegram/QQ/WebUI.

### Files in WebUI

In an active WebUI session, use the **attach** (📎) button next to the input to upload a file (optional caption in the input box). Uploads are stored under `~/.cc-gateway/media/` and shown in chat; when the agent session is running, the file is forwarded to the agent like bot inbound media. Agents can push files back via MCP `send_file` — they appear in the same chat thread and download from `/api/media/{filename}`.

## Daemon Mode

The daemon enforces a single instance by binding to a local port (`port` in config, default `17534`). If another daemon is already running, `start` will report the existing PID instead of spawning a second process.

### Start

```sh
cc-gateway start
```

Starts cc-gateway as a background daemon. The daemon listens for messages from all enabled platforms (Feishu, Telegram, and QQ can run simultaneously).

### Stop

```sh
cc-gateway stop
```

Gracefully shuts down the daemon. All active chat sessions receive a shutdown notice and each Claude subprocess is given 500 ms to exit before being forcefully terminated.

### Restart

```sh
cc-gateway restart
```

### View Logs

```sh
cc-gateway log              # Show last 100 lines
cc-gateway log -f           # Follow log output
cc-gateway log -n 500       # Show last 500 lines
```

## Pairing new chats

When `require_pairing` is enabled (default), open **WebUI → Pairing** after a user messages the bot for the first time. The bot sends a pairing code; approve the request before `/agent` works. See [bots/README.md](bots/README.md).

## Feishu Bot

Setup: [bots/feishu.md](bots/feishu.md). Once the daemon is running with Feishu configured, you can:

1. Open Feishu and find your bot
2. Send messages directly — they are forwarded to Claude Code when a session is active
3. Use gateway commands: `/cd`, `/agent`, `/agents`, `/agent-history`, `/pwd`, `/ll`, `/help`, `/quit`

Each chat (group or private) gets its own isolated Claude subprocess, so messages from different chats never mix.

### Directory Selection Card

Send `/ll` in Feishu to receive an interactive card listing folders from `default_dir`. Tap a folder button to change the working directory.

### Command Boundaries in Feishu

- `/cd ..` can only navigate up to `default_dir` — attempting to go above it returns an access denied message
- `/quit` is only valid when a Claude session is active; otherwise you will receive a message提示

## Telegram Bot

Setup: [bots/telegram.md](bots/telegram.md). Once the daemon is running with Telegram configured:

1. Open Telegram and find your bot
2. Send messages directly — they are forwarded to Claude Code
3. Use the same gateway commands as in WebUI

Each chat gets its own isolated agent subprocess. Telegram uses long-polling (`getUpdates`) only.

## QQ Bot

Setup: [bots/qq.md](bots/qq.md). Once the daemon is running with QQ configured:

1. **Private (C2C):** DM the bot directly.
2. **Group:** @ the bot when sending a message.
3. Use the same gateway commands (`/agent`, `/cd`, `/help`, …).
4. `/ll` and agent selection are **plain text** (no interactive cards).

Each QQ channel (`u:…` / `g:…`) has its own isolated agent session. Restart the daemon after changing QQ credentials or `sandbox`.

## Tips

- Use `/agent-history` to list recent sessions, then `/agent-history <n>` to resume by index
- Keep sensitive credentials in environment variables, not in config.json
- If the default port is occupied by another program, change `port` in `config.json` or let the install script auto-detect a free port
