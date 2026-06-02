use std::path::PathBuf;

use crate::runtime::session::remove_mcp_config_file;

#[test]
fn remove_mcp_config_file_deletes_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cc-gateway-mcp-test.json");
    std::fs::write(&path, r#"{"mcpServers":{}}"#).unwrap();
    assert!(path.is_file());

    remove_mcp_config_file(Some(&path));

    assert!(!path.exists());
}

#[test]
fn remove_mcp_config_file_is_noop_when_none() {
    remove_mcp_config_file(None::<&PathBuf>);
}
