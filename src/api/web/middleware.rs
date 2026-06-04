use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::IntoResponse,
};
use std::net::{Ipv4Addr, SocketAddr};

use crate::web::handlers::session::AppState;

/// Check whether an IPv4 address matches a CIDR notation string.
///
/// Supports both bare IPs ("127.0.0.1") and prefix notation ("192.168.1.0/24").
fn ip_matches_cidr(ip: &Ipv4Addr, cidr: &str) -> bool {
    // Bare IP (no prefix) — exact match
    let (network_str, prefix_len) = match cidr.split_once('/') {
        Some((net, len)) => {
            let len: u8 = match len.parse() {
                Ok(v) if v <= 32 => v,
                _ => return false,
            };
            (net, len)
        }
        None => {
            let network: Ipv4Addr = match cidr.parse() {
                Ok(addr) => addr,
                Err(_) => return false,
            };
            return *ip == network;
        }
    };

    let network: Ipv4Addr = match network_str.parse() {
        Ok(addr) => addr,
        Err(_) => return false,
    };

    let ip_u32 = u32::from(*ip);
    let net_u32 = u32::from(network);
    let mask = if prefix_len == 0 {
        0
    } else {
        !0u32 << (32 - prefix_len)
    };

    (ip_u32 & mask) == (net_u32 & mask)
}

/// Check whether any CIDR in the allowlist matches `ip`.
fn is_ip_allowed(ip: &Ipv4Addr, allowed_ips: &[String]) -> bool {
    if allowed_ips.is_empty() {
        return true; // empty allowlist = allow all
    }
    allowed_ips.iter().any(|cidr| ip_matches_cidr(ip, cidr))
}

/// Axum middleware that rejects requests from IPs not in the configured
/// CIDR allowlist.
///
/// When `allowed_ips` is empty, all IPs are permitted (default).
pub async fn ip_allowlist(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    // Resolve to IPv4; skip check for non-IPv4 addresses (allow by default).
    if let SocketAddr::V4(ref v4) = addr {
        let ip = v4.ip();
        if !is_ip_allowed(ip, &state.allowed_ips) {
            tracing::warn!(
                "[ip_allowlist] Rejected request from {} (not in allowlist)",
                ip
            );
            return Err((
                StatusCode::FORBIDDEN,
                r#"{"code":403,"msg":"access denied by IP allowlist"}"#.to_string(),
            ));
        }
    }
    Ok(next.run(request).await)
}

/// Generate a cryptographically-random 32-character hex token.
pub fn generate_webui_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Extract a token from request: query param `?token=xxx` takes priority,
/// then `Authorization: Bearer xxx` header.
fn extract_token(request: &Request) -> Option<String> {
    // Check query param first
    if let Some(token) = request.uri().query().and_then(|q| {
        q.split('&')
            .find(|p| p.starts_with("token="))
            .map(|p| &p[6..])
    }) {
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }

    // Then check Authorization: Bearer header
    let headers = request.headers();
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        if let Some(token) = auth.strip_prefix("Bearer ") {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Axum middleware that rejects requests without a valid WebUI token.
///
/// When `webui_token` is `None`, all requests are permitted (backwards compatible).
/// When set, requests under `/api` must include the token via `?token=xxx` query
/// param or `Authorization: Bearer xxx` header. Static assets and the HTML page
/// are exempt from token checks.
///
/// On successful token auth, a long-lived cookie is set so the browser persists
/// the token across sessions — even when localStorage is unavailable.
pub async fn webui_token_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> impl IntoResponse {
    if let Some(ref expected) = state.webui_token {
        if request.uri().path().starts_with("/api") {
            let token_valid = extract_token(&request).is_some_and(|t| t == *expected);
            if !token_valid {
                tracing::warn!("[webui_token] Rejected request (invalid or missing token)");
                return Err((
                    StatusCode::UNAUTHORIZED,
                    r#"{"code":401,"msg":"missing or invalid token"}"#.to_string(),
                ));
            }
            // Token is valid — allow and set persistent cookie
            let response = next.run(request).await;
            let (mut parts, body) = response.into_parts();
            if let Ok(cookie_val) = HeaderValue::from_str(&format!(
                "cc_gateway_token={}; Path=/; Max-Age=31536000; SameSite=Lax",
                expected
            )) {
                parts.headers.insert(header::SET_COOKIE, cookie_val);
            }
            let response = axum::response::Response::from_parts(parts, body);
            return Ok(response);
        }
    }
    Ok(next.run(request).await)
}
