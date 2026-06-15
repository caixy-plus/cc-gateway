//! Agent subsystem: Abstracts various local CLI agents (`claude`, `codex-acp`, `agent`, `opencode`,
//! `kimi`, `gemini`, `qoderclicn`, `pi`) into a unified "agent session".
//!
//! # Submodule Responsibilities
//!
//! - [`session`]: The [`AgentRuntime`](session::AgentRuntime) enum, dispatching to different backends based on the provider.
//! - [`backend`]: The [`AgentBackend`](backend::AgentBackend) trait and the `dispatch_agent_backend!` macro,
//!   defining all capabilities of the gateway over "agent sessions" (sending messages, switching models, compacting context, stopping, etc.).
//! - [`acp_session`] / [`acp_client`]: Shared implementation of the stdio NDJSON JSON-RPC-based **ACP** protocol.
//!   Codex, Cursor, OpenCode, Kimi, Gemini, and **Qoder** all connect through a thin `AcpHooks` implementation.
//! - [`codex_acp`] / [`cursor_acp`] / [`opencode_acp`] / [`kimi_acp`] / [`gemini_acp`] / [`qoder_acp`]:
//!   Hook implementations for each ACP provider.
//! - [`pi_rpc`]: Pi **JSON-RPC over stdio** client (Pi uses its own custom protocol, not ACP).
//! - [`mcp_attach`]: Injection logic for gateway-level MCP servers (mounting capabilities like `send_file` to the provider).
//! - [`event`]: Unified [`AgentEvent`](event::AgentEvent) event stream across all providers.

pub mod acp_client;
pub mod acp_session;
pub mod backend;
pub mod codex_acp;
pub mod cursor_acp;
pub mod event;
pub mod gemini_acp;
pub mod kimi_acp;
pub mod mcp_attach;
pub mod opencode_acp;
pub mod pi_rpc;
pub mod qoder_acp;
pub mod session;

/// Constructs the list of environment variables passed to the child process.
///
/// The gateway itself runs in the user's shell environment and passes most environment variables through to the provider child processes.
/// However, a few "provider-exclusive" environment variables must not contaminate other providers. For example, `CLAUDECODE`
/// is used internally by Claude Code to identify whether its parent process is Claude Code. Leaking it to other
/// providers might cause those CLIs to incorrectly assume they are running inside a Claude Code child process, causing erratic behavior.
///
/// Other provider-specific variables (such as `QODER_PERSONAL_ACCESS_TOKEN`, `GEMINI_API_KEY`, `OPENAI_API_KEY`,
/// etc.) are **passed through by default**. This is the only way for the provider to obtain credentials if the user
/// does not specify environment variables in `config.json`. To configure credentials per provider, they should
/// be explicitly overridden in `agent.providers.<id>.env`.
pub fn passthrough_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| k != "CLAUDECODE")
        .collect()
}

/// Create a [`tokio::process::Command`] for spawning an agent CLI subprocess.
///
/// On Windows, sets `CREATE_NO_WINDOW` so no console window is shown on the
/// desktop for each spawned agent session. `.cmd` / `.bat` entrypoints are
/// launched via `cmd /C` so npm-style wrappers spawn correctly.
pub fn agent_command(cli_path: &str) -> tokio::process::Command {
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let lower = cli_path.to_lowercase();
        if lower.ends_with(".cmd") || lower.ends_with(".bat") {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/C").arg(cli_path);
            cmd.creation_flags(CREATE_NO_WINDOW);
            return cmd;
        }
        let mut cmd = tokio::process::Command::new(cli_path);
        cmd.creation_flags(CREATE_NO_WINDOW);
        return cmd;
    }
    #[cfg(not(windows))]
    {
        tokio::process::Command::new(cli_path)
    }
}
