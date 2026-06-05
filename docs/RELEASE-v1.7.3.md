# Release v1.7.3 (draft notes)

Use this body when publishing `v1.7.3` on GitHub (edit via `gh release edit` if CI auto-creates the release).

- **新增 QQ 官方机器人平台** / Add QQ Open Platform official bot (WebSocket Gateway, C2C, pairing, MCP `send_file`) — 配置见 `docs/bots/qq.md` / See `docs/bots/qq.md`. *(Later releases: group @ chat is unsupported; C2C only — see current `docs/bots/qq.md`.)*
- **新增 OpenCode agent 支持** / Add OpenCode agent provider (`opencode acp`) — 在 `config.json` → `agent.providers.opencode` 中启用，使用 `/agent opencode` / Enable via `agent.providers.opencode` in `config.json`, use `/agent opencode`.
- **文档与安装脚本** / Docs and install scripts list all bot guides and the platform integration checklist under `docs/bots/`.
- **WebUI**（独立仓库 `cc-gateway-webui`）/ WebUI sibling repo: QQ settings section — 发版前需同步构建并 push WebUI / build and push WebUI before tagging.

## Manual verification (operators)

1. `cc-gateway init` → enable QQ or OpenCode as needed.
2. QQ: `restart`, pairing, `/agent`, C2C vs group `send_file`.
3. OpenCode: `opencode auth login`, `/agents`, permission text flow on QQ/Telegram.
