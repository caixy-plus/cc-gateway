use crate::platform::feishu::handle::{
    build_http_response, extract_post_content, split_text_into_chunks,
};
use crate::platform::feishu::{DedupCache, RateLimiter};

#[test]
fn feishu_extracts_plain_and_post_content_with_images() {
    let (plain, images) = extract_post_content(r#"{"text":"hello"}"#);
    assert_eq!(plain, "hello");
    assert!(images.is_empty());

    let post = serde_json::json!({
        "title": "Title",
        "content": [[
            {"tag": "text", "text": "body"},
            {"tag": "a", "text": "link"},
            {"tag": "at", "user_name": "Alice"},
            {"tag": "img", "image_key": "img-key"}
        ]]
    });
    let (text, images) = extract_post_content(&post.to_string());

    assert_eq!(text, "Title\nbody\nlink\n@Alice");
    assert_eq!(images, vec!["img-key"]);
}

#[test]
fn feishu_splits_long_text_without_splitting_utf8_chars() {
    let chunks = split_text_into_chunks("你好世界", 2);

    assert_eq!(chunks, vec!["你好", "世界"]);
    assert!(chunks.iter().all(|chunk| chunk.chars().count() <= 2));
}

#[test]
fn feishu_http_response_sets_status_content_length_and_body() {
    let body = r#"{"ok":true}"#;
    let response = build_http_response(429, body);

    assert!(response.starts_with("HTTP/1.1 429 Too Many Requests\r\n"));
    assert!(response.contains(&format!("Content-Length: {}", body.len())));
    assert!(response.ends_with(body));
}

#[test]
fn feishu_dedup_cache_and_rate_limiter_cover_webhook_guards() {
    let dedup = DedupCache::new(60);
    dedup.insert("msg-1".to_string());
    assert!(dedup.contains("msg-1"));
    assert!(!dedup.contains("msg-2"));

    let limiter = RateLimiter::new(2, 60);
    assert!(limiter.check("ip"));
    assert!(limiter.check("ip"));
    assert!(!limiter.check("ip"));
    assert!(limiter.check("other-ip"));
}
