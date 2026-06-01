//! ACP client-side filesystem RPC (`read_text_file` / `write_text_file`).
//!
//! When `initialize` advertises `clientCapabilities.fs`, the agent calls back into
//! cc-gateway (the ACP client) for file I/O scoped to the session `cwd`.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::process::ChildStdin;
use tokio::sync::Mutex;

use crate::agent::acp_client::{spawn_jsonrpc_error, spawn_jsonrpc_result};

/// Handle agent→client filesystem methods. Returns `true` if `msg` was consumed.
pub fn try_handle_fs_request(msg: &Value, work_dir: &str, stdin: &Arc<Mutex<ChildStdin>>) -> bool {
    let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
        return false;
    };
    let Some(id) = msg.get("id").cloned() else {
        return false;
    };
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    match normalize_fs_method(method) {
        Some(FsMethod::Read) => {
            let work = work_dir.to_string();
            let stdin = stdin.clone();
            tokio::spawn(async move {
                let result = match read_text_file(&work, &params) {
                    Ok(content) => json!({ "content": content }),
                    Err(e) => {
                        spawn_jsonrpc_error(&stdin, id, -32000, e.to_string());
                        return;
                    }
                };
                spawn_jsonrpc_result(&stdin, id, result);
            });
            true
        }
        Some(FsMethod::Write) => {
            let work = work_dir.to_string();
            let stdin = stdin.clone();
            tokio::spawn(async move {
                if let Err(e) = write_text_file(&work, &params) {
                    spawn_jsonrpc_error(&stdin, id, -32000, e.to_string());
                    return;
                }
                spawn_jsonrpc_result(&stdin, id, Value::Null);
            });
            true
        }
        None => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FsMethod {
    Read,
    Write,
}

fn normalize_fs_method(method: &str) -> Option<FsMethod> {
    match method {
        "read_text_file" | "fs/read_text_file" => Some(FsMethod::Read),
        "write_text_file" | "fs/write_text_file" => Some(FsMethod::Write),
        _ => None,
    }
}

fn path_from_params(params: &Value) -> Option<&str> {
    params
        .get("path")
        .or_else(|| params.get("uri"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
}

pub fn resolve_path_under_work_dir(work_dir: &str, path: &str) -> Result<PathBuf> {
    let work = PathBuf::from(work_dir);
    let input = PathBuf::from(path);
    let joined = if input.is_absolute() {
        input
    } else {
        work.join(input)
    };
    let canonical = joined.canonicalize().unwrap_or_else(|_| joined.clone());
    let work_canon = work.canonicalize().unwrap_or(work);
    if !path_starts_with(&canonical, &work_canon) {
        anyhow::bail!(
            "path {} is outside session working directory {}",
            canonical.display(),
            work_canon.display()
        );
    }
    Ok(canonical)
}

fn normalize_path_for_compare(path: &str) -> String {
    let path = path.trim_end_matches('/');
    if let Some(rest) = path.strip_prefix("/private") {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// `starts_with` that tolerates macOS `/var` vs `/private/var` after canonicalize.
fn path_starts_with(path: &PathBuf, base: &PathBuf) -> bool {
    let path = normalize_path_for_compare(&path.to_string_lossy());
    let base = normalize_path_for_compare(&base.to_string_lossy());
    path == base || path.starts_with(&format!("{}/", base))
}

pub fn read_text_file(work_dir: &str, params: &Value) -> Result<String> {
    let path_str = path_from_params(params).context("missing path in read_text_file")?;
    let file_path = resolve_path_under_work_dir(work_dir, path_str)?;

    if file_path.is_dir() {
        let mut items: Vec<String> = std::fs::read_dir(&file_path)
            .with_context(|| format!("failed to list {}", file_path.display()))?
            .filter_map(|e| e.ok())
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let suffix = if e.path().is_dir() { "/" } else { "" };
                format!("  {}{}", name, suffix)
            })
            .collect();
        items.sort();
        if items.is_empty() {
            return Ok(format!("[Directory {} is empty]", file_path.display()));
        }
        items.insert(0, format!("Contents of {}:", file_path.display()));
        return Ok(items.join("\n"));
    }

    let content = std::fs::read_to_string(&file_path)
        .with_context(|| format!("failed to read {}", file_path.display()))?;

    let line: usize = params
        .get("line")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .unwrap_or(1)
        .max(1) as usize;
    let limit: Option<usize> = params
        .get("limit")
        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
        .map(|n| n as usize);

    Ok(slice_lines(&content, line, limit))
}

pub fn write_text_file(work_dir: &str, params: &Value) -> Result<()> {
    let path_str = path_from_params(params).context("missing path in write_text_file")?;
    let content = params
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let file_path = resolve_path_under_work_dir(work_dir, path_str)?;
    if let Some(parent) = file_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create parent dir {}", parent.display()))?;
        }
    }
    std::fs::write(&file_path, content)
        .with_context(|| format!("failed to write {}", file_path.display()))?;
    Ok(())
}

fn slice_lines(content: &str, start_line: usize, limit: Option<usize>) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let start = start_line.saturating_sub(1);
    let end = match limit {
        Some(n) => (start + n).min(lines.len()),
        None => lines.len(),
    };
    if start >= lines.len() {
        return String::new();
    }
    lines[start..end].join("\n")
}

/// Read file content for legacy `<acp:read_file>` tags embedded in assistant text.
pub fn read_for_acp_tag(work_dir: &str, path: &str, offset: usize, limit: Option<usize>) -> String {
    let params = json!({
        "path": path,
        "line": offset,
        "limit": limit,
    });
    read_text_file(work_dir, &params).unwrap_or_else(|e| format!("[Error reading {}: {}]", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn slice_lines_respects_offset_and_limit() {
        let text = "a\nb\nc\nd";
        assert_eq!(slice_lines(text, 2, Some(2)), "b\nc");
    }

    #[test]
    fn resolve_path_rejects_escape_from_work_dir() {
        let dir = std::env::temp_dir().join(format!("acp-fs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let sub = dir.join("proj");
        fs::create_dir_all(&sub).unwrap();
        let work = sub.to_str().unwrap();
        fs::write(sub.join("ok.txt"), "1").unwrap();
        assert!(resolve_path_under_work_dir(work, "ok.txt").is_ok());
        assert!(resolve_path_under_work_dir(work, "/etc/passwd").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_and_write_roundtrip_under_work_dir() {
        let dir = std::env::temp_dir().join(format!("acp-fs-rw-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let work = dir.to_str().unwrap();
        write_text_file(
            work,
            &json!({ "path": "out.txt", "content": "hello\nworld" }),
        )
        .unwrap();
        let got =
            read_text_file(work, &json!({ "path": "out.txt", "line": 2, "limit": 1 })).unwrap();
        assert_eq!(got, "world");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_fs_method_names() {
        assert_eq!(
            normalize_fs_method("fs/read_text_file"),
            Some(FsMethod::Read)
        );
        assert_eq!(
            normalize_fs_method("write_text_file"),
            Some(FsMethod::Write)
        );
    }
}
