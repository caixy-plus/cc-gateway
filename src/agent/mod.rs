pub mod acp_client;
pub mod acp_fs;
pub mod codewhale_acp;
pub mod codewhale_context;
pub mod cursor_acp;
pub mod event;
pub mod mcp_attach;
pub mod opencode_acp;
pub mod pi_rpc;
pub mod session;

/// Collect environment variables for child processes, filtering out
/// provider-specific vars (e.g. CLAUDECODE) that shouldn't leak across agents.
pub fn passthrough_env() -> Vec<(String, String)> {
    std::env::vars()
        .filter(|(k, _)| k != "CLAUDECODE")
        .collect()
}
