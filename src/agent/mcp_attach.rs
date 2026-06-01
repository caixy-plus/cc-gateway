//! Attach cc-gateway's `send_file` MCP server to agent providers when supported.

use std::path::PathBuf;

use anyhow::Result;
use serde_json::{json, Value};
use tracing::debug;

use crate::config::model::AgentProvider;
use crate::runtime::file_delivery::{McpContext, MCP_TARGET_ENV};

/// How a provider can receive [`McpContext`] for Feishu/Telegram file delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMcpSupport {
    /// Claude Code `--mcp-config` JSON file.
    ClaudeMcpConfig,
    /// ACP `session/new` / `session/load` `mcpServers` array (Cursor, OpenCode).
    AcpSession,
    /// Not wired yet (Pi RPC has no MCP hook in cc-gateway).
    Unsupported,
}

pub fn provider_mcp_support(provider: AgentProvider) -> ProviderMcpSupport {
    match provider {
        AgentProvider::Claude => ProviderMcpSupport::ClaudeMcpConfig,
        AgentProvider::Cursor | AgentProvider::OpenCode => ProviderMcpSupport::AcpSession,
        AgentProvider::Pi => ProviderMcpSupport::Unsupported,
    }
}

pub fn supports_mcp_attach(provider: AgentProvider) -> bool {
    !matches!(
        provider_mcp_support(provider),
        ProviderMcpSupport::Unsupported
    )
}

/// Claude `--mcp-config` file body (`mcpServers` object map).
pub fn build_claude_mcp_servers_object(mcp_context: &McpContext) -> Result<Value> {
    let target_json = mcp_context.to_env_json()?;
    let command = gateway_executable_path();
    Ok(json!({
        "cc-gateway": {
            "command": command,
            "args": ["_mcp-server"],
            "env": {
                (MCP_TARGET_ENV): target_json,
            }
        }
    }))
}

/// ACP [`NewSessionRequest`](https://agentclientprotocol.com/protocol/session-setup) `mcpServers` array.
pub fn build_acp_mcp_servers(mcp_context: Option<&McpContext>) -> Result<Value> {
    let Some(ctx) = mcp_context else {
        return Ok(json!([]));
    };
    if !supports_mcp_attach_for_context(ctx) {
        return Ok(json!([]));
    }
    let target_json = ctx.to_env_json()?;
    let command = gateway_executable_path();
    Ok(json!([{
        "name": "cc-gateway",
        "command": command,
        "args": ["_mcp-server"],
        "env": [
            { "name": MCP_TARGET_ENV, "value": target_json }
        ]
    }]))
}

fn gateway_executable_path() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("cc-gateway"))
        .to_string_lossy()
        .into_owned()
}

fn supports_mcp_attach_for_context(_ctx: &McpContext) -> bool {
    true
}

/// Tool names used when providers surface MCP `send_file` permission prompts.
pub fn is_gateway_send_file_tool(tool_name: &str) -> bool {
    tool_name == "mcp__cc-gateway__send_file"
        || tool_name == "send_file"
        || tool_name == "cc-gateway__send_file"
        || (tool_name.contains("cc-gateway") && tool_name.contains("send_file"))
}

pub fn log_unsupported_mcp(provider: AgentProvider) {
    debug!(
        provider = %provider,
        "MCP send_file attach is not implemented for this provider; Feishu/Telegram file delivery via agent tools is unavailable"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::file_delivery::{FeishuFileTarget, McpDeliveryTarget};

    fn sample_context() -> McpContext {
        McpContext {
            delivery: McpDeliveryTarget::Feishu(FeishuFileTarget {
                app_id: "app".to_string(),
                app_secret: "secret".to_string(),
                chat_id: "chat".to_string(),
                receive_id_type: "open_id".to_string(),
            }),
        }
    }

    #[test]
    fn acp_mcp_servers_empty_without_context() {
        let servers = build_acp_mcp_servers(None).unwrap();
        assert_eq!(servers, json!([]));
    }

    #[test]
    fn acp_mcp_servers_includes_cc_gateway_stdio_server() {
        let servers = build_acp_mcp_servers(Some(&sample_context())).unwrap();
        let arr = servers.as_array().expect("array");
        assert_eq!(arr.len(), 1);
        assert_eq!(
            arr[0].get("name").and_then(|v| v.as_str()),
            Some("cc-gateway")
        );
        assert!(arr[0].get("command").and_then(|v| v.as_str()).is_some());
        let args = arr[0].get("args").and_then(|v| v.as_array()).unwrap();
        assert_eq!(args[0], json!("_mcp-server"));
        let env = arr[0].get("env").and_then(|v| v.as_array()).unwrap();
        assert_eq!(
            env[0].get("name").and_then(|v| v.as_str()),
            Some(MCP_TARGET_ENV)
        );
    }

    #[test]
    fn provider_mcp_support_matrix() {
        assert_eq!(
            provider_mcp_support(AgentProvider::Claude),
            ProviderMcpSupport::ClaudeMcpConfig
        );
        assert_eq!(
            provider_mcp_support(AgentProvider::Cursor),
            ProviderMcpSupport::AcpSession
        );
        assert_eq!(
            provider_mcp_support(AgentProvider::OpenCode),
            ProviderMcpSupport::AcpSession
        );
        assert_eq!(
            provider_mcp_support(AgentProvider::Pi),
            ProviderMcpSupport::Unsupported
        );
    }

    #[test]
    fn gateway_send_file_tool_name_matching() {
        assert!(is_gateway_send_file_tool("mcp__cc-gateway__send_file"));
        assert!(is_gateway_send_file_tool("send_file"));
        assert!(!is_gateway_send_file_tool("Bash"));
    }
}
