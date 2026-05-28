use crate::platform::inbound_media::{
    format_agent_message, generate_storage_filename, SavedInboundMedia,
};
use std::path::PathBuf;

#[test]
fn storage_filename_is_uuid_with_extension() {
    let a = generate_storage_filename(Some("image/png"));
    let b = generate_storage_filename(Some("image/png"));
    assert_ne!(a, b);
    assert!(a.ends_with(".png"));
    let stem = a.strip_suffix(".png").unwrap();
    assert!(uuid::Uuid::parse_str(stem).is_ok());
}

#[test]
fn format_uses_empty_alt_image_markdown() {
    let full = "/home/u/.cc-gateway/media/550e8400-e29b-41d4-a716-446655440000.png";
    let items = vec![SavedInboundMedia {
        path: PathBuf::from(full),
        is_image: true,
    }];
    let msg = format_agent_message("notes", &items);
    assert!(msg.contains("notes"));
    assert!(msg.contains(&format!("![]({full})")));
}
