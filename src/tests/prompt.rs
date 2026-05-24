use crate::prompt::{load_default_prompt, load_prompt_from_file};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_load_default_prompt_contains_key_sections() {
    let prompt = load_default_prompt();
    assert!(prompt.contains("cc-gateway"));
    assert!(prompt.contains("/cd"));
    assert!(prompt.contains("/pwd"));
    assert!(prompt.contains("/ll"));
    assert!(prompt.contains("/allow"));
    assert!(prompt.contains("/deny"));
    assert!(prompt.contains("MCP Bash tool"));
    assert!(prompt.contains("safety"));
}

#[test]
fn test_load_default_prompt_is_non_empty() {
    let prompt = load_default_prompt();
    assert!(!prompt.is_empty());
}

#[test]
fn test_load_prompt_from_file_success() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "Custom prompt content").unwrap();
    let path = temp.path();
    let result = load_prompt_from_file(path);
    assert!(result.is_some());
    assert!(result.unwrap().contains("Custom prompt content"));
}

#[test]
fn test_load_prompt_from_file_missing() {
    let result = load_prompt_from_file("/nonexistent/path/to/prompt.txt");
    assert!(result.is_none());
}

#[test]
fn test_load_prompt_from_file_empty() {
    let temp = NamedTempFile::new().unwrap();
    let result = load_prompt_from_file(temp.path());
    assert!(result.is_some());
    assert_eq!(result.unwrap(), "");
}
