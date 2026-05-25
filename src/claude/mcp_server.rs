use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, error, info, warn};

/// Context for MCP (Model Context Protocol) server that allows Claude Code
/// subprocesses to send files back to their bound chat sessions.
#[derive(Debug, Clone)]
pub struct McpContext {
    pub feishu_app_id: String,
    pub feishu_app_secret: String,
    pub chat_id: String,
    pub receive_id_type: String,
}

/// Known file extensions and their Feishu file_type mapping.
fn detect_file_type(path: &str) -> &str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "pdf" => "pdf",
        "doc" => "doc",
        "docx" => "docx",
        "xls" => "xls",
        "xlsx" => "xlsx",
        "ppt" => "ppt",
        "pptx" => "pptx",
        "csv" => "csv",
        "txt" | "md" | "rs" | "py" | "js" | "ts" | "json" | "toml" | "yaml" | "yml" | "log"
        | "html" | "css" | "sh" | "bash" | "sql" => "stream",
        "mp4" | "mov" | "avi" => "mp4",
        "opus" | "ogg" => "opus",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => "image",
        _ => "stream",
    }
}

/// Run an internal MCP server for Claude Code via stdio JSON-RPC 2.0.
/// Invoked via `cc-gateway _mcp-server` (hidden CLI command).
pub async fn run_mcp_server() -> Result<()> {
    // Read context from environment variables (injected by session.rs MCP config)
    let app_id = std::env::var("CC_GATEWAY_FEISHU_APP_ID").unwrap_or_default();
    let app_secret = std::env::var("CC_GATEWAY_FEISHU_APP_SECRET").unwrap_or_default();
    let chat_id = std::env::var("CC_GATEWAY_FEISHU_CHAT_ID").unwrap_or_default();
    let receive_id_type = std::env::var("CC_GATEWAY_FEISHU_RECEIVE_ID_TYPE").unwrap_or_default();

    info!(
        "MCP server starting: app_id={}, chat_id={}, receive_id_type={}",
        app_id, chat_id, receive_id_type
    );

    if app_id.is_empty() || app_secret.is_empty() || chat_id.is_empty() {
        error!("MCP server missing required environment variables");
        return Ok(());
    }

    // Build a minimal FeishuPlatform for API calls
    let config = crate::config::model::FeishuConfig {
        app_id: app_id.clone(),
        app_secret: app_secret.clone(),
        allow_from: "*".to_string(),
        encrypt_key: String::new(),
        enabled: true,
        mode: "websocket".to_string(),
        webhook_bind: String::new(),
    };
    let platform = crate::platform::feishu::FeishuPlatform::new(
        config,
        "~",
        crate::config::model::ClaudeConfig::default(),
        false,
    );

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
                    "tools": [
                        {
                            "name": "send_file",
                            "description": "发送本地文件到当前飞书聊天",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "本地文件的绝对路径"
                                    },
                                    "file_name": {
                                        "type": "string",
                                        "description": "可选，显示的文件名，默认取 path 的 basename"
                                    }
                                },
                                "required": ["path"]
                            }
                        }
                    ]
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
                    match handle_send_file(&platform, &chat_id, &receive_id_type, &arguments).await
                    {
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

async fn handle_send_file(
    platform: &crate::platform::feishu::FeishuPlatform,
    chat_id: &str,
    receive_id_type: &str,
    arguments: &Value,
) -> Result<Value> {
    let path_str = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("缺少 path 参数"))?;

    let expanded = shellexpand::tilde(path_str).to_string();
    let path = std::path::Path::new(&expanded);

    // Check file exists
    if !path.exists() {
        anyhow::bail!("文件不存在: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!("路径不是文件: {}", path.display());
    }

    // Check file size ≤ 30MB
    let metadata = std::fs::metadata(path)?;
    let max_size: u64 = 30 * 1024 * 1024;
    if metadata.len() > max_size {
        anyhow::bail!("文件太大: {} bytes (最大 30MB)", metadata.len());
    }
    if metadata.len() == 0 {
        anyhow::bail!("不允许发送空文件");
    }

    // Determine file_name
    let file_name = arguments
        .get("file_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string()
        });

    let file_type = detect_file_type(&path_str);
    let file_data = tokio::fs::read(path).await?;

    info!(
        "MCP send_file: path={}, file_name={}, file_type={}, size={}",
        path.display(),
        file_name,
        file_type,
        file_data.len()
    );

    // Upload to Feishu
    let file_key = platform
        .upload_file(file_type, &file_name, file_data)
        .await?;
    info!("MCP send_file: uploaded, file_key={}", file_key);

    // Send file message
    let message_id = platform
        .send_file_message(receive_id_type, chat_id, &file_key)
        .await?;
    info!(
        "MCP send_file: sent, message_id={}, file_key={}",
        message_id, file_key
    );

    Ok(json!({
        "file_key": file_key,
        "message_id": message_id,
        "file_name": file_name
    }))
}
