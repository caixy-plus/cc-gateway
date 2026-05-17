# Usage Guide

## Interactive Mode

Run `cc-gateway` without any subcommand to enter interactive chat mode:

```bash
$ cc-gateway
cc-gateway interactive mode  Type '/help' for commands, '/quit' to exit.

cc-gateway> /claude
Claude session started in: /Users/you/Workspace

💬 ~/Workspace ▶ hello Claude
Hello! How can I help you today?

💬 ~/Workspace ▶ /quit
Claude session stopped.

cc-gateway> /quit
```

### Command Completion

Press `Tab` after typing `/` to see a list of available commands with inline descriptions.

### Session Switching

- `/claude` — enters Claude session mode. The prompt changes to `💬 ~/Workspace ▶`
- In session mode, everything you type goes directly to Claude
- `/quit` — stops the session and returns to gateway mode
- When not in a session, `/quit` exits the program entirely

### Directory Navigation

```bash
cc-gateway> /cd ~/Projects/my-app
Working directory changed to: /Users/you/Projects/my-app

cc-gateway> /ll
# Opens an interactive TUI directory picker
# Use ↑↓ to navigate, Enter to cd, q to cancel
```

## Daemon Mode

### Start

```bash
cc-gateway start
```

Starts cc-gateway as a background daemon. The daemon listens for Feishu messages (if configured).

### Stop

```bash
cc-gateway stop
```

### Restart

```bash
cc-gateway restart
```

### View Logs

```bash
cc-gateway log              # Show last 100 lines
cc-gateway log -f           # Follow log output
cc-gateway log -n 500       # Show last 500 lines
```

## Feishu Bot

Once the daemon is running with Feishu configured, you can:

1. Open Feishu and find your bot
2. Send messages directly — they are forwarded to Claude Code when a session is active
3. Use gateway commands just like in CLI mode: `/cd`, `/claude`, `/pwd`, `/ll`, `/help`, `/quit`

### Directory Selection Card

Send `/ll` in Feishu to receive an interactive card listing folders from `default_dir`. Tap a folder button to change the working directory.

### Command Boundaries in Feishu

- `/cd ..` can only navigate up to `feishu.default_dir` — attempting to go above it returns an access denied message
- `/quit` is only valid when a Claude session is active; otherwise you will receive a message提示

## Tips

- Use `/claude --resume <id>` to resume a previous Claude session
- Keep sensitive credentials in environment variables, not in config.json
