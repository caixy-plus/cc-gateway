use axum::{
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use crate::config::model::GatewayConfig;
use crate::web::handlers;
use crate::web::handlers::session::AppState;

pub fn create_app(config: &GatewayConfig) -> Router {
    let state = AppState {
        claude_config: config.claude.clone(),
        show_thinking: config.show_thinking,
        default_dir: config.default_dir.clone(),
    };

    Router::new()
        // API routes
        .route("/api/cmd/ll", post(handlers::cmd::handle_ll))
        .route("/api/cmd/pwd", post(handlers::cmd::handle_pwd))
        .route("/api/cmd/cd", post(handlers::cmd::handle_cd))
        .route("/api/cmd/cd_default", post(handlers::cmd::handle_cd_default))
        .route("/api/cmd/help", post(handlers::cmd::handle_help))
        .route("/api/cmd/show-thinking-toggle", post(handlers::cmd::handle_show_thinking_toggle))
        .route("/api/deliver", post(handlers::deliver::handle_deliver))
        .route("/api/sessions", get(handlers::session::handle_list_sessions))
        .route("/api/sessions", post(handlers::session::handle_create_session))
        .route("/api/sessions/{id}/messages", post(handlers::session::handle_send_message))
        .route("/api/sessions/{id}", post(handlers::session::handle_stop_session))
        .route("/api/sessions/{id}", delete(handlers::session::handle_delete_session))
        .route("/api/sessions/{id}/history", get(handlers::session::handle_get_history))
        .route("/api/sessions/{id}/events", get(handlers::session::handle_events))
        .route("/api/config", get(handlers::config::handle_get_config))
        .route("/api/config", post(handlers::config::handle_save_config))
        .route("/api/platforms", get(handlers::config::handle_get_platforms))
        // WebUI static files
        .route("/", get(handlers::ui::serve_index))
        .route("/{*path}", get(handlers::ui::serve_static))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
