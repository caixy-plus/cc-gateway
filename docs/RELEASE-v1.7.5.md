# Release v1.7.5

- **修复 OpenCode 会话恢复** / Fix OpenCode session resume when `session/load` omits `sessionId` in the ACP response — reuse the requested session id
- **ACP 恢复加固** / Harden Cursor/OpenCode ACP spawn with shared `resolve_acp_spawn_session_id`
- **恢复前工作目录** / On resume, sync channel `work_dir` to the stored session directory before spawning the provider
- **友好错误提示** / Localized user-facing messages for spawn/resume failures (ACP timeout, process exit, disabled provider, session not found)
- **Telegram 恢复失败提示** / Telegram resume callback now shows friendly errors instead of silent failure
