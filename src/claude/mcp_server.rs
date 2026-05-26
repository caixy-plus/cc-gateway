use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

use crate::claude::file_delivery::{
    validate_outbound_file, FileDelivery, MAX_OUTBOUND_FILE_BYTES, MCP_TARGET_ENV,
};

pub use crate::claude::file_delivery::McpContext;

pub(crate) fn send_file_tool_schema() -> Value {
    let max_mb = MAX_OUTBOUND_FILE_BYTES / 1024 / 1024;
    json!({
        "name": "send_file",
        "description": format!("发送本地文件到当前聊天。文件大小上限为 {}MB，超过限制请先压缩、拆分或按 ABI 等方式生成更小文件。", max_mb),
        "inputSchema": {
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": format!("本地文件的绝对路径；文件必须存在且不超过 {}MB", max_mb)
                },
                "file_name": {
                    "type": "string",
                    "description": "可选，显示的文件名，默认取 path 的 basename"
                }
            },
            "required": ["path"]
        }
    })
}

/// Run an internal MCP server for Claude Code via stdio JSON-RPC 2.0.
/// Invoked via `cc-gateway _mcp-server` (hidden CLI command).
pub async fn run_mcp_server() -> Result<()> {
    let target_json = std::env::var(MCP_TARGET_ENV).unwrap_or_default();
    if target_json.is_empty() {
        error!("MCP server missing delivery target");
        return Ok(());
    }
    let context = McpContext::from_env_json(&target_json)?;
    info!("MCP server starting with delivery target");

    // JSON-RPC 2.0 loop on stdin/stdout
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();
    let write = Arc::new(tokio::sync::Mutex::new(stdout));

    while let Some(line) = lines
        .next_line()
        .await
        .context("Failed to read stdin line")?
    {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        debug!("MCP stdin: {}", line);

        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                warn!("MCP invalid JSON: {}", e);
                continue;
            }
        };

        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "cc-gateway",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            "notifications/initialized" => {
                // No response needed for notifications
                continue;
            }
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [send_file_tool_schema()]
                }
            }),
            "tools/call" => {
                let params = request.get("params").cloned().unwrap_or(json!({}));
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

                if tool_name != "send_file" {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": format!("Unknown tool: {}", tool_name)
                        }
                    })
                } else {
                    match handle_send_file(&context, &arguments).await {
                        Ok(result) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": serde_json::to_string(&result).unwrap_or_default()
                                    }
                                ]
                            }
                        }),
                        Err(e) => json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": {
                                "content": [
                                    {
                                        "type": "text",
                                        "text": format!("发送文件失败: {}", e)
                                    }
                                ],
                                "isError": true
                            }
                        }),
                    }
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": format!("Unknown method: {}", method)
                }
            }),
        };

        let response_str = serde_json::to_string(&response).unwrap_or_default();
        debug!("MCP stdout: {}", response_str);

        let mut w = write.lock().await;
        w.write_all(response_str.as_bytes()).await?;
        w.write_all(b"\n").await?;
        w.flush().await?;
    }

    info!("MCP server shutting down");
    Ok(())
}

async fn handle_send_file(context: &McpContext, arguments: &Value) -> Result<Value> {
    let path_str = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("缺少 path 参数"))?;
    let file_name = arguments
        .get("file_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty());

    let outbound = validate_outbound_file(path_str, file_name).await?;

    info!(
        "MCP send_file: path={}, file_name={}, file_type={}, size={}",
        outbound.path.display(),
        outbound.file_name,
        outbound.file_type,
        outbound.bytes.len()
    );

    let sent = context.delivery.send_file(outbound).await?;
    info!("MCP send_file: sent via {}", sent.platform);

    serde_json::to_value(sent).context("Failed to serialize sent file result")
}
