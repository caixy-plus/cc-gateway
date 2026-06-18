# 聊天平台接入检查清单

在 cc-gateway 中**新增或大幅改动**聊天机器人平台时，请使用本清单。可复制到 PR 描述中逐项勾选。**必填项未勾选不得合并**。

[English](platform-integration-checklist.md) | 简体中文

## 功能对齐参考（当前平台）

| 能力 | 飞书 | Telegram |
|------|------|----------|
| `Platform`（`run` / `shutdown`） | 有 | 有 |
| 配置 + `runtime_defaults()` | 有 | 有 |
| Daemon 经 `platform_registry` 启动 | 有 | 有 |
| `SessionSource` + DB `source` | 有 | 有 |
| 配对（`require_pairing`） | 有 | 有 |
| `ChatCommandExecutor` + `CommandRouter` | 有 | 有 |
| `EventPollSink`（流式回复） | 有 | 有 |
| **MCP `send_file`** | 有 | 有 |
| 命令路径 `McpContext` | 有 | 有 |
| Deliver-bus 文本推送 | 有 | 有 |
| WebUI 配置 + `/api/platforms` | 有 | 有 |
| Init 向导机器人步骤 | 有 | 有 |
| i18n（`<platform>.*`） | 有 | 有 |
| 入站媒体转 agent | 有 | 有 |
| `/ll`、`/agents` 交互 | 卡片 | 内联按钮 |
| 权限确认 UI | 卡片/回调 | 内联按钮 |
| 无会话时未知命令 | 定制帮助 | 帮助文本 |

若有意的 **无**，须在 `docs/bots/<id>.zh-CN.md` 中说明。

---

## A. 后端代码（必填）

| # | 项 | 文件 / 说明 |
|---|-----|-------------|
| A1 | `GatewayConfig` 配置结构 | `src/core/config/model.rs` |
| A2 | `Default` + `runtime_defaults()` | `model.rs` |
| A3 | 重启/热更新字段路径 | `src/core/config/platform_registry.rs` + `restart_policy.rs` |
| A4 | 平台模块 `src/platform/<name>/` | 实现 `Platform` |
| A5 | `pub mod <name>` | `src/platform.rs` |
| A6 | `PlatformDef` 注册 + daemon 启动 | `src/core/config/platform_registry.rs`、`src/daemon/engine.rs` |
| A7 | 连接状态 | `src/platform/status.rs` |
| A8 | `SessionSource` | `src/core/session/channel_model.rs`、`src/database.rs`、`channel_manager.rs` |
| A9 | `with_mcp_context` | 平台入站处理 |
| A10 | `McpDeliveryTarget` + `FileDelivery` | `src/core/runtime/file_delivery.rs` |
| A11 | `EventPollSink` | 平台根文件 `platform/<name>.rs` |
| A12 | `spawn_deliver_listener`（如需） | `platform.rs` |
| A13 | Web 配置读写 / 平台列表 | `src/api/web/handlers/config.rs` |
| A14 | Init 向导 | `src/core/config/wizard.rs` |
| A15 | i18n 中英文 | `src/utils/i18n/dict.rs` |
| A16 | 测试 | 单元测试写在对应 `.rs` 末尾；仅跨模块流程测试放 `src/tests/` + `tests.rs` |

## B. Platform Reference Docs（必填）

在 [docs/platform-reference.md](platform-reference.md) 增加 **`## <平台>`**，包含：

- 开发者控制台  
- 鉴权文档  
- 实际传输方式（WS / 轮询 / Webhook）  
- 入站事件名  
- 出站消息 API  
- 富媒体 / 文件 API（供 MCP）  
- 官方文档根链接  

## C. 用户文档（必填）

| # | 项 |
|---|-----|
| C1 | `docs/bots/<platform>.md` + `.zh-CN.md` |
| C2 | `docs/bots/README` 中英文（含 MCP 列） |
| C3 | `docs/config` 中英文 |
| C4 | `docs/usage` 中英文 |
| C5 | `README` 中英文 |
| C6 | `scripts/install-docs.*` |
| C7 | `CLAUDE.md` 平台列表与 MCP 矩阵 |

## D. 前端 `../cc-gateway-webui`（必填，直至平台 API 动态化）

| # | 项 |
|---|-----|
| D1 | `types/index.ts` |
| D2 | `SettingsModal.tsx` |
| D3 | `i18n` 文案 |
| D4 | 会话列表 / 配对 UI |

## E. 验证（必填）

1. 相关 `cargo test`  
2. `cc-gateway init`  
3. `cc-gateway start`，日志 / WebUI 在线  
4. 配对流程  
5. `/agent`、对话、`/cd`、`/ll`、`/quit`  
6. MCP `send_file`（若支持）  
7. 改凭证需 restart；`require_pairing` 可热更新  
8. 安装脚本输出含新文档链接  

---

## 智能体（非聊天平台）

见 [docs/adding-agent-provider.md](adding-agent-provider.md)，更新 registry、`docs/config`、README，一般**不需要** `docs/bots/`。
