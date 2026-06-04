use axum::{
    extract::Path,
    http::{header, StatusCode},
    response::{Html, IntoResponse},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "webui/dist/"]
struct Assets;

pub async fn serve_index() -> Html<String> {
    let html = Assets::get("index.html")
        .map(|f| String::from_utf8_lossy(&f.data).to_string())
        .unwrap_or_else(|| {
            r#"<!DOCTYPE html><html><body style="font-family:monospace;padding:40px">
<h2>cc-gateway WebUI</h2>
<p>Frontend build artifacts not embedded.</p>
<p>Build the frontend first:</p>
<pre>cd cc-gateway-webui && npm run build:embed</pre>
</body></html>"#
                .to_string()
        });
    Html(html)
}

pub async fn serve_static(Path(path): Path<String>) -> impl axum::response::IntoResponse {
    let path = path.trim_start_matches('/');
    let mime = mime_guess::from_path(path).first_or_text_plain();
    match Assets::get(path) {
        Some(file) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data.to_vec(),
        )
            .into_response(),
        None => serve_index().await.into_response(),
    }
}
