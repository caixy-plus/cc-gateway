use crate::cli::interactive::format_banner;

// ------------------------------------------------------------------
// Format helper tests
// ------------------------------------------------------------------

#[test]
fn test_format_banner_contains_mode() {
    let s = format_banner();
    assert!(s.contains("interactive mode"));
    assert!(s.contains("/help"));
}
