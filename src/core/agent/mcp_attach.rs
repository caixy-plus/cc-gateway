//! Attach cc-gateway's `send_file` MCP server to agent providers when supported.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::debug;

use crate::config::model::AgentProvider;
use crate::runtime::file_delivery::{McpContext, MCP_TARGET_ENV};

const CURSOR_MCP_DIR: &str = ".cursor";
const PI_MCP_DIR: &str = ".pi";
const GATEWAY_MCP_SERVER_NAME: &str = "cc-gateway";

/// How a provider can receive [`McpContext`] for Feishu/Telegram file delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMcpSupport {
    /// Claude Code `--mcp-config` JSON file.
    ClaudeMcpConfig,
    /// ACP `session/new` / `session/load` `mcpServers` array (OpenCode).
    AcpSession,
    /// Project-level `mcp.json` under `.cursor/` or `.pi/` (Cursor, Pi).
    ProjectMcpJson,
}

pub fn provider_mcp_support(provider: AgentProvider) -> ProviderMcpSupport {
    match provider {
        AgentProvider::Claude => ProviderMcpSupport::ClaudeMcpConfig,
        AgentProvider::OpenCode => ProviderMcpSupport::AcpSession,
        AgentProvider::Cursor | AgentProvider::Pi => ProviderMcpSupport::ProjectMcpJson,
    }
}

pub fn supports_mcp_attach(_provider: AgentProvider) -> bool {
    true
}

/// One stdio MCP server entry for project-level `mcp.json` files.
pub fn build_project_mcp_server_entry(mcp_context: &McpContext) -> Result<Value> {
    let target_json = mcp_context.to_env_json()?;
    let command = gateway_executable_path();
    Ok(json!({
        "command": command,
        "transport": "stdio",
        "args": ["_mcp-server"],
        "env": {
            (MCP_TARGET_ENV): target_json,
        }
    }))
}

/// Claude `--mcp-config` file body (`mcpServers` object map).
pub fn build_claude_mcp_servers_object(mcp_context: &McpContext) -> Result<Value> {
    Ok(json!({
        (GATEWAY_MCP_SERVER_NAME): build_project_mcp_server_entry(mcp_context)?,
    }))
}

/// Write or merge cc-gateway into `{work_dir}/.cursor/mcp.json` or `{work_dir}/.pi/mcp.json`.
pub async fn write_project_mcp_json(
    work_dir: &str,
    provider: AgentProvider,
    mcp_context: &McpContext,
) -> Result<PathBuf> {
    let subdir = match provider {
        AgentProvider::Cursor => CURSOR_MCP_DIR,
        AgentProvider::Pi => PI_MCP_DIR,
        other => anyhow::bail!("project MCP json is not supported for {other}"),
    };
    let expanded = shellexpand::tilde(work_dir).to_string();
    let dir = PathBuf::from(&expanded).join(subdir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("Failed to create {}", dir.display()))?;
    let path = dir.join("mcp.json");

    let mut servers = if path.is_file() {
        let content = tokio::fs::read_to_string(&path).await?;
        serde_json::from_str::<Value>(&content)
            .ok()
            .and_then(|v| v.get("mcpServers").cloned())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    } else {
        serde_json::Map::new()
    };

    servers.insert(
        GATEWAY_MCP_SERVER_NAME.to_string(),
        build_project_mcp_server_entry(mcp_context)?,
    );
    let body = json!({ "mcpServers": Value::Object(servers) });
    tokio::fs::write(&path, serde_json::to_string_pretty(&body)?)
        .await
        .with_context(|| format!("Failed to write {}", path.display()))?;
    info_project_mcp_written(provider, &path);
    Ok(path)
}

fn info_project_mcp_written(provider: AgentProvider, path: &Path) {
    debug!(
        provider = %provider,
        path = %path.display(),
        "Wrote cc-gateway MCP config for provider"
    );
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
        "type": "stdio",
        "name": GATEWAY_MCP_SERVER_NAME,
        "command": command,
        "args": ["_mcp-server"],
        "env": [
            { "name": MCP_TARGET_ENV, "value": target_json }
        ]
    }]))
}

/// Cursor ACP reads MCP from project `.cursor/mcp.json`; also pass stdio servers on session/new.
pub async fn prepare_cursor_mcp(work_dir: &str, mcp_context: Option<&McpContext>) -> Result<Value> {
    let Some(ctx) = mcp_context else {
        return Ok(json!([]));
    };
    write_project_mcp_json(work_dir, AgentProvider::Cursor, ctx).await?;
    build_acp_mcp_servers(Some(ctx))
}

/// Pi loads MCP from project `.pi/mcp.json` (requires pi-mcp-adapter or compatible extension).
pub async fn prepare_pi_mcp(work_dir: &str, mcp_context: Option<&McpContext>) -> Result<()> {
    if let Some(ctx) = mcp_context {
        write_project_mcp_json(work_dir, AgentProvider::Pi, ctx).await?;
    }
    Ok(())
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
            ProviderMcpSupport::ProjectMcpJson
        );
        assert_eq!(
            provider_mcp_support(AgentProvider::OpenCode),
            ProviderMcpSupport::AcpSession
        );
        assert_eq!(
            provider_mcp_support(AgentProvider::Pi),
            ProviderMcpSupport::ProjectMcpJson
        );
    }

    #[test]
    fn acp_mcp_servers_use_stdio_transport_type() {
        let servers = build_acp_mcp_servers(Some(&sample_context())).unwrap();
        assert_eq!(
            servers[0].get("type").and_then(|v| v.as_str()),
            Some("stdio")
        );
    }

    #[tokio::test]
    async fn write_project_mcp_json_for_cursor_and_pi() {
        let dir = std::env::temp_dir().join(format!("cc-gateway-mcp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let ctx = sample_context();

        let cursor_path =
            write_project_mcp_json(dir.to_str().unwrap(), AgentProvider::Cursor, &ctx)
                .await
                .unwrap();
        assert_eq!(cursor_path, dir.join(".cursor/mcp.json"));
        let cursor_body = std::fs::read_to_string(&cursor_path).unwrap();
        assert!(cursor_body.contains("cc-gateway"));
        assert!(cursor_body.contains("_mcp-server"));

        let pi_path = write_project_mcp_json(dir.to_str().unwrap(), AgentProvider::Pi, &ctx)
            .await
            .unwrap();
        assert_eq!(pi_path, dir.join(".pi/mcp.json"));
        let pi_body = std::fs::read_to_string(&pi_path).unwrap();
        assert!(pi_body.contains("cc-gateway"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gateway_send_file_tool_name_matching() {
        assert!(is_gateway_send_file_tool("mcp__cc-gateway__send_file"));
        assert!(is_gateway_send_file_tool("send_file"));
        assert!(!is_gateway_send_file_tool("Bash"));
    }
}
