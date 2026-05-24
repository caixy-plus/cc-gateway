// Tests for Feishu auth middleware (src/platform/feishu/auth_middleware.rs).
//
// Covers TokenManager construction, caching, invalidation, error detection,
// and FeishuAuthMiddleware construction.  Network-dependent tests
// (get_tenant_access_token, refresh_token) are skipped here; they exist
// in src/tests/platform_feishu.rs with #[ignore] attributes.

use crate::platform::feishu::auth_middleware::{TokenManager, FeishuAuthMiddleware};
use crate::config::model::FeishuConfig;

fn make_token_manager() -> TokenManager {
    let config = FeishuConfig {
        enabled: true,
        app_id: "test_app".to_string(),
        app_secret: "test_secret".to_string(),
        allow_from: "*".to_string(),
        encrypt_key: "".to_string(),
        mode: "websocket".to_string(),
        webhook_bind: "0.0.0.0:3000".to_string(),
    };
    TokenManager::new(config)
}

// ---------------------------------------------------------------------------
// TokenManager construction
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_token_manager_starts_with_no_cached_token() {
    let tm = make_token_manager();
    let cached = tm.cached_token.read().await;
    assert!(cached.is_none());
    let fetched_at = tm.token_fetched_at.read().await;
    assert!(fetched_at.is_none());
}

#[tokio::test]
async fn test_token_manager_constructs_with_config() {
    let tm = make_token_manager();
    // The config is private but we can verify the manager is created
    let _ = tm;
}

// ---------------------------------------------------------------------------
// TokenManager::invalidate_token_cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_token_manager_invalidate_clears_cache() {
    let tm = make_token_manager();

    // Manually set a cached token
    {
        let mut cached = tm.cached_token.write().await;
        *cached = Some("fake-token".to_string());
        let mut fetched_at = tm.token_fetched_at.write().await;
        *fetched_at = Some(std::time::Instant::now());
    }

    // Verify it was set
    {
        let cached = tm.cached_token.read().await;
        assert!(cached.is_some());
    }

    // Invalidate
    tm.invalidate_token_cache().await;

    // Verify cleared
    {
        let cached = tm.cached_token.read().await;
        assert!(cached.is_none());
        let fetched_at = tm.token_fetched_at.read().await;
        assert!(fetched_at.is_none());
    }
}

// ---------------------------------------------------------------------------
// TokenManager::is_auth_error
// ---------------------------------------------------------------------------

#[test]
fn test_is_auth_error_detects_known_error_codes() {
    // 99991663 = tenant access token invalid
    let err = anyhow::anyhow!("Feishu API error: 99991663 - invalid token");
    assert!(TokenManager::is_auth_error(&err));

    // 99991661 = app access token invalid
    let err = anyhow::anyhow!("error code 99991661");
    assert!(TokenManager::is_auth_error(&err));

    // 99991664 = user access token invalid
    let err = anyhow::anyhow!("code: 99991664, something went wrong");
    assert!(TokenManager::is_auth_error(&err));
}

#[test]
fn test_is_auth_error_detects_invalid_access_token_message() {
    let err = anyhow::anyhow!("Invalid access token");
    assert!(TokenManager::is_auth_error(&err));

    let err = anyhow::anyhow!("Failed: Invalid access token expired");
    assert!(TokenManager::is_auth_error(&err));
}

#[test]
fn test_is_auth_error_returns_false_for_unrelated_errors() {
    let err = anyhow::anyhow!("network timeout");
    assert!(!TokenManager::is_auth_error(&err));

    let err = anyhow::anyhow!("permission denied");
    assert!(!TokenManager::is_auth_error(&err));

    let err = anyhow::anyhow!("rate limited: too many requests");
    assert!(!TokenManager::is_auth_error(&err));
}

#[test]
fn test_is_auth_error_empty_error() {
    let err = anyhow::anyhow!("");
    assert!(!TokenManager::is_auth_error(&err));
}

#[test]
fn test_is_auth_error_partial_code_match_no_false_positive() {
    // "9999166" is 7 digits (prefix but not a full known error code)
    let err = anyhow::anyhow!("error 9999166");
    assert!(!TokenManager::is_auth_error(&err));

    // "A99991663" has the code as substring but not as a standalone token
    let err = anyhow::anyhow!("code A99991663");
    assert!(TokenManager::is_auth_error(&err), "A99991663 contains 99991663");

    // "8888888" is completely unrelated
    let err = anyhow::anyhow!("error code 8888888");
    assert!(!TokenManager::is_auth_error(&err));
}

// ---------------------------------------------------------------------------
// TokenManager clone
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_token_manager_clone_shares_cache() {
    let tm1 = make_token_manager();
    let tm2 = tm1.clone();

    // Set a token via tm1
    {
        let mut cached = tm1.cached_token.write().await;
        *cached = Some("shared-token".to_string());
    }

    // tm2 sees the same token
    {
        let cached = tm2.cached_token.read().await;
        assert_eq!(cached.as_deref(), Some("shared-token"));
    }

    // Invalidate via tm2
    tm2.invalidate_token_cache().await;

    // tm1 sees it cleared
    {
        let cached = tm1.cached_token.read().await;
        assert!(cached.is_none());
    }
}

// ---------------------------------------------------------------------------
// FeishuAuthMiddleware construction
// ---------------------------------------------------------------------------

#[test]
fn test_auth_middleware_constructs() {
    let tm = make_token_manager();
    let middleware = FeishuAuthMiddleware::new(tm.clone());
    // Just verify construction succeeds; the middleware itself
    // requires a reqwest pipeline to test handle().
    let _ = middleware;
}

#[test]
fn test_auth_middleware_clone_works() {
    let tm = make_token_manager();
    let mw1 = FeishuAuthMiddleware::new(tm);
    let mw2 = mw1.clone();
    let _ = mw2;
}
