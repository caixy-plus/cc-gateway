# Release v1.7.4

- **新增 QQ 官方机器人** / Add QQ Open Platform bot (WebSocket Gateway, C2C + group @, pairing, MCP `send_file` with group media limits) — `docs/bots/qq.md`
- **新增 OpenCode 智能体** / Add OpenCode agent provider (`opencode acp`) — enable in `agent` profiles, `/agent opencode`
- **移除 CodeWhale** / Remove CodeWhale provider (ACP integration and docs); legacy `codewhale` / `codew` config keys are stripped on load
- **文档与安装** / Bot setup guides (`docs/bots/`), platform integration checklist, `install-docs` scripts
- **WebUI** / Sibling repo `cc-gateway-webui`: QQ settings (push before using release binaries that embed WebUI from CI)
