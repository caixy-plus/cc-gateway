use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

/// Context for configuring the MCP server when spawning a Claude session.
#[derive(Debug, Clone)]
pub struct McpContext {
    pub feishu_app_id: String,
    pub feishu_app_secret: String,
    pub chat_id: String,
    pub receive_id_type: String,
}

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn error(id: Option<Value>, code: i32, message: &str) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Feishu file uploader
// ---------------------------------------------------------------------------

struct FeishuUploader {
    app_id: String,
    app_secret: String,
    chat_id: String,
    receive_id_type: String,
    http_client: reqwest::Client,
    cached_token: tokio::sync::Mutex<Option<(String, std::time::Instant)>>,
}

impl FeishuUploader {
    fn new(app_id: String, app_secret: String, chat_id: String, receive_id_type: String) -> Self {
        Self {
            app_id,
            app_secret,
            chat_id,
            receive_id_type,
            http_client: reqwest::Client::new(),
            cached_token: tokio::sync::Mutex::new(None),
        }
    }

    async fn get_token(&self) -> Result<String> {
        {
            let cached = self.cached_token.lock().await;
            if let Some((ref token, instant)) = *cached {
                if instant.elapsed().as_secs() < 3300 {
                    return Ok(token.clone());
                }
            }
        }

        let resp = self
            .http_client
            .post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .context("Failed to fetch tenant access token")?;

        let body: Value = resp.json().await.context("Failed to parse token response")?;
        let code = body.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = body.get("msg").and_then(|v| v.as_str()).unwrap_or("unknown");
            anyhow::bail!("Feishu token API error: code={}, msg={}", code, msg);
        }

        let token = body
            .get("tenant_access_token")
            .and_then(|v| v.as_str())
            .context("Missing tenant_access_token")?
            .to_string();

        let mut cached = self.cached_token.lock().await;
        *cached = Some((token.clone(), std::time::Instant::now()));
        Ok(token)
    }

    fn detect_file_type(path: &str) -> &str {
        let ext = Path::new(path)
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
            "txt" => "txt",
            "mp4" => "mp4",
            "opus" => "opus",
            _ => "stream",
        }
    }

    fn msg_type_for_file_type(file_type: &str) -> &str {
        match file_type {
            "mp4" => "media",
            "opus" => "audio",
            _ => "file",
        }
    }

    async fn upload_and_send(&self, path: &str, display_name: &str) -> Result<Value> {
        // Validate file
        let file_path = Path::new(path);
        if !file_path.exists() {
            anyhow::bail!("File not found: {}", path);
        }
        if !file_path.is_file() {
            anyhow::bail!("Path is not a file: {}", path);
        }

        let metadata = std::fs::metadata(path).context("Failed to read file metadata")?;
        let file_size = metadata.len();
        if file_size > 30 * 1024 * 1024 {
            anyhow::bail!(
                "File too large: {} bytes (max 30MB)",
                file_size
            );
        }
        if file_size == 0 {
            anyhow::bail!("File is empty");
        }

        let file_data = tokio::fs::read(path)
            .await
            .context("Failed to read file")?;

        let file_type = Self::detect_file_type(path);
        let msg_type = Self::msg_type_for_file_type(file_type);
        let token = self.get_token().await?;

        // Step 1: Upload file to Feishu
        let file_part = reqwest::multipart::Part::bytes(file_data)
            .file_name(display_name.to_string())
            .mime_str("application/octet-stream")
            .context("Failed to create multipart part")?;

        let form = reqwest::multipart::Form::new()
            .text("file_type", file_type.to_string())
            .text("file_name", display_name.to_string())
            .part("file", file_part);

        let upload_resp = self
            .http_client
            .post("https://open.feishu.cn/open-apis/im/v1/files")
            .bearer_auth(&token)
            .multipart(form)
            .send()
            .await
            .context("Failed to upload file to Feishu")?;

        let upload_body: Value = upload_resp
            .json()
            .await
            .context("Failed to parse upload response")?;
        let upload_code = upload_body
            .get("code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if upload_code != 0 {
            let msg = upload_body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!(
                "Feishu file upload failed: code={}, msg={}",
                upload_code,
                msg
            );
        }

        let file_key = upload_body
            .get("data")
            .and_then(|d| d.get("file_key"))
            .and_then(|v| v.as_str())
            .context("Missing file_key in upload response")?
            .to_string();

        info!(
            "Uploaded file to Feishu: file_key={}, file_type={}, file_name={}",
            file_key, file_type, display_name
        );

        // Step 2: Send file message
        let send_body = serde_json::json!({
            "receive_id": self.chat_id,
            "msg_type": msg_type,
            "content": serde_json::to_string(&serde_json::json!({
                "file_key": file_key
            })).unwrap_or_default(),
        });

        let send_resp = self
            .http_client
            .post("https://open.feishu.cn/open-apis/im/v1/messages")
            .bearer_auth(&token)
            .query(&[("receive_id_type", &self.receive_id_type)])
            .json(&send_body)
            .send()
            .await
            .context("Failed to send file message")?;

        let send_body: Value = send_resp
            .json()
            .await
            .context("Failed to parse send message response")?;
        let send_code = send_body
            .get("code")
            .and_then(|v| v.as_i64())
            .unwrap_or(-1);
        if send_code != 0 {
            let msg = send_body
                .get("msg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!(
                "Feishu send file message failed: code={}, msg={}",
                send_code,
                msg
            );
        }

        let message_id = send_body
            .get("data")
            .and_then(|d| d.get("message_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        info!(
            "Sent file message: file_key={}, message_id={:?}",
            file_key, message_id
        );

        Ok(serde_json::json!({
            "file_key": file_key,
            "message_id": message_id,
            "file_name": display_name,
            "file_type": file_type,
            "file_size": file_size,
        }))
    }
}

// ---------------------------------------------------------------------------
// MCP request handler
// ---------------------------------------------------------------------------

async fn handle_request(req: &JsonRpcRequest, uploader: &Option<FeishuUploader>) -> Option<JsonRpcResponse> {
    match req.method.as_str() {
        "initialize" => {
            let result = serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "cc-gateway",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            });
            Some(JsonRpcResponse::success(req.id.clone(), result))
        }
        "notifications/initialized" => {
            // Notification — no response needed
            None
        }
        "tools/list" => {
            let tools = serde_json::json!({
                "tools": [{
                    "name": "send_file",
                    "description": "发送本地文件到当前飞书聊天。支持 pdf/doc/docx/xls/xlsx/ppt/pptx/csv/txt/mp4/opus 等格式，文件大小不超过30MB。",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "本地文件的绝对路径"
                            },
                            "file_name": {
                                "type": "string",
                                "description": "可选，显示的文件名，默认取 path 的文件名"
                            }
                        },
                        "required": ["path"]
                    }
                }]
            });
            Some(JsonRpcResponse::success(req.id.clone(), tools))
        }
        "tools/call" => {
            let params = match &req.params {
                Some(p) => p,
                None => {
                    return Some(JsonRpcResponse::error(
                        req.id.clone(),
                        -32602,
                        "Missing params",
                    ));
                }
            };

            let tool_name = params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if tool_name != "send_file" {
                return Some(JsonRpcResponse::error(
                    req.id.clone(),
                    -32601,
                    &format!("Unknown tool: {}", tool_name),
                ));
            }

            let arguments = params.get("arguments").unwrap_or(&Value::Null);
            let path = arguments
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let file_name = arguments
                .get("file_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("file")
                        .to_string()
                });

            if path.is_empty() {
                return Some(JsonRpcResponse::error(
                    req.id.clone(),
                    -32602,
                    "Missing required parameter: path",
                ));
            }

            match uploader {
                Some(ref u) => match u.upload_and_send(path, &file_name).await {
                    Ok(result) => {
                        let text = format!(
                            "文件已发送成功！\nfile_key: {}\nmessage_id: {}\n文件名: {}\n文件类型: {}\n文件大小: {} bytes",
                            result["file_key"].as_str().unwrap_or(""),
                            result["message_id"].as_str().unwrap_or(""),
                            result["file_name"].as_str().unwrap_or(""),
                            result["file_type"].as_str().unwrap_or(""),
                            result["file_size"].as_u64().unwrap_or(0),
                        );
                        let mcp_result = serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": text
                            }],
                            "isError": false
                        });
                        Some(JsonRpcResponse::success(req.id.clone(), mcp_result))
                    }
                    Err(e) => {
                        let mcp_result = serde_json::json!({
                            "content": [{
                                "type": "text",
                                "text": format!("文件发送失败: {}", e)
                            }],
                            "isError": true
                        });
                        Some(JsonRpcResponse::success(req.id.clone(), mcp_result))
                    }
                },
                None => {
                    let mcp_result = serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": "send_file 仅在飞书平台可用。当前环境缺少飞书配置。"
                        }],
                        "isError": true
                    });
                    Some(JsonRpcResponse::success(req.id.clone(), mcp_result))
                }
            }
        }
        "ping" => {
            Some(JsonRpcResponse::success(req.id.clone(), serde_json::json!({})))
        }
        _ => Some(JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            &format!("Method not found: {}", req.method),
        )),
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_mcp_server() -> Result<()> {
    let app_id = std::env::var("CC_GATEWAY_FEISHU_APP_ID").ok();
    let app_secret = std::env::var("CC_GATEWAY_FEISHU_APP_SECRET").ok();
    let chat_id = std::env::var("CC_GATEWAY_FEISHU_CHAT_ID").ok();
    let receive_id_type = std::env::var("CC_GATEWAY_FEISHU_RECEIVE_ID_TYPE")
        .unwrap_or_else(|_| "chat_id".to_string());

    let uploader = match (app_id, app_secret, chat_id) {
        (Some(id), Some(secret), Some(cid)) => Some(FeishuUploader::new(id, secret, cid, receive_id_type)),
        _ => {
            warn!("MCP server started without Feishu credentials — send_file will not work");
            None
        }
    };

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    info!("MCP server started, waiting for requests...");

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                info!("MCP server stdin closed, exiting");
                break;
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                debug!("MCP ← {}", trimmed);

                let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("Failed to parse MCP request: {} (input: {})", e, trimmed);
                        continue;
                    }
                };

                if let Some(resp) = handle_request(&req, &uploader).await {
                    let json = serde_json::to_string(&resp).unwrap_or_default();
                    debug!("MCP → {}", json);
                    stdout.write_all(json.as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
            }
            Err(e) => {
                warn!("Error reading MCP stdin: {}", e);
                break;
            }
        }
    }

    Ok(())
}
