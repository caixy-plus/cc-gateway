/// Mask a credential for API responses (WebUI round-trip).
pub fn mask_secret(s: &str) -> String {
    if s.len() <= 8 {
        "***".to_string()
    } else {
        format!("{}***{}", &s[..4], &s[s.len() - 4..])
    }
}

/// True when `incoming` is the masked form of `existing_secret` from [`mask_secret`].
pub fn is_masked_secret(incoming: &str, existing_secret: &str) -> bool {
    if incoming.is_empty() {
        return false;
    }
    incoming == mask_secret(existing_secret)
}
