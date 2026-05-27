# Usage Guide

## Interactive Mode

Run `cc-gateway` without any subcommand to enter interactive chat mode:

```sh
$ cc-gateway
cc-gateway interactive mode  Type '/help' for commands, '/quit' to exit.

cc-gateway> /agent
agent session started in: /Users/you/Workspace

💬 ~/Workspace ▶ hello Claude
Hello! How can I help you today?

💬 ~/Workspace ▶ /quit
agent session stopped.

cc-gateway> /quit
```

### Command Completion

Press `Tab` after typing `/` to see a list of available commands with inline descriptions.

### Session Switching

- `/agent` — enters agent session mode. The prompt changes to `💬 ~/Workspace ▶`
- In session mode, everything you type goes directly to the active agent
- `/quit` — stops the session and returns to gateway mode
- When not in a session, `/quit` exits the program entirely

### Directory Navigation

```sh
cc-gateway> /cd ~/Projects/my-app
Working directory changed to: /Users/you/Projects/my-app

cc-gateway> /ll
# Opens an interactive TUI directory picker
# Use ↑↓ to navigate, Enter to cd, q to cancel
```

## Daemon Mode

The daemon enforces a single instance by binding to a local port (`port` in config, default `17534`). If another daemon is already running, `start` will report the existing PID instead of spawning a second process.

### Start

```sh
cc-gateway start
```

Starts cc-gateway as a background daemon. The daemon listens for messages from all enabled platforms (Feishu and Telegram can run simultaneously).

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

## Feishu Bot

Once the daemon is running with Feishu configured, you can:

1. Open Feishu and find your bot
2. Send messages directly — they are forwarded to Claude Code when a session is active
3. Use gateway commands just like in CLI mode: `/cd`, `/agent`, `/agents`, `/agent-history`, `/pwd`, `/ll`, `/help`, `/quit`

Each chat (group or private) gets its own isolated Claude subprocess, so messages from different chats never mix.

### Directory Selection Card

Send `/ll` in Feishu to receive an interactive card listing folders from `default_dir`. Tap a folder button to change the working directory.

### Command Boundaries in Feishu

- `/cd ..` can only navigate up to `default_dir` — attempting to go above it returns an access denied message
- `/quit` is only valid when a Claude session is active; otherwise you will receive a message提示

## Telegram Bot

Once the daemon is running with Telegram configured:

1. Open Telegram and find your bot
2. Send messages directly — they are forwarded to Claude Code
3. Use the same gateway commands as in CLI mode

Each chat gets its own isolated Claude subprocess. The Telegram platform uses long-polling (`getUpdates`) by default; set `webhook_url` to switch to webhook mode.

## Tips

- Use `/agent-history` to list recent sessions, then `/agent-history <n>` to resume by index
- Keep sensitive credentials in environment variables, not in config.json
- If the default port is occupied by another program, change `port` in `config.json` or let the install script auto-detect a free port
