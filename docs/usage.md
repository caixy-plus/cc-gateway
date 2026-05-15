# Usage Guide

## Interactive Mode

Run `cc-gateway` without any subcommand to enter interactive chat mode:

```bash
$ cc-gateway
cc-gateway interactive mode
Type '/help' for available commands, '/quit' to exit.

cc-gateway> /claude
Claude session started in: /Users/you/Workspace

cc-gateway> hello Claude
Hello! How can I help you today?

cc-gateway> /pwd
Current directory: /Users/you/Workspace

cc-gateway> /cd ~/Projects/my-app
Working directory changed to: /Users/you/Projects/my-app

cc-gateway> /cc-quit
Claude session stopped.

cc-gateway> /quit
```

## Daemon Mode

### Start

```bash
cc-gateway start
```

Starts cc-gateway as a background daemon. The daemon listens for:
- Feishu messages (if configured)
- CLI commands via `cc-gateway send` (future)

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
2. Send messages directly - they are forwarded to Claude Code
3. Use gateway commands just like in CLI mode: `/cd`, `/claude`, `/pwd`, etc.

### Permission Requests

When Claude Code asks for permission to use a tool, you'll receive a message in Feishu. Reply with:
- `allow` or `yes` or `允许` - Approve the request
- `deny` or `no` or `拒绝` - Deny the request

## Project Auto-Detection

If `ai.enabled` is true in your config, cc-gateway will try to understand which project you want to work on:

**You:** "帮我修一下 gateway 的 bug"

**cc-gateway:** "Detected project: ~/Projects/gateway. Start working here? (yes/no)"

**You:** "yes"

**cc-gateway:** "Working directory changed to: ~/Projects/gateway. Claude session started."

## Tips

- Use `/cc/clear` to clear Claude's conversation history
- Use `/cc/compact` to compress long conversations
- Use `/model sonnet` to switch to Claude Sonnet model
- Keep sensitive credentials in environment variables, not in config.json
