use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use tower_http::cors::CorsLayer;

use crate::config::model::GatewayConfig;
use crate::web::handlers;
use crate::web::handlers::session::AppState;

use crate::web::middleware as auth_middleware;

#[allow(dead_code)]
pub fn create_app(config: &GatewayConfig) -> Router {
    create_app_with_config_path(config, None)
}

pub fn create_app_with_config_path(
    config: &GatewayConfig,
    config_path: Option<std::path::PathBuf>,
) -> Router {
    let state = AppState {
        agent_settings: config.effective_agent_settings(),
        show_thinking: config.show_thinking,
        default_dir: config.default_dir.clone(),
        daemon_config_path: config_path,
        allowed_ips: config.allowed_ips.clone(),
        webui_token: config.webui_token.clone(),
    };

    Router::new()
        // API routes
        .route("/api/cmd/ll", post(handlers::cmd::handle_ll))
        .route("/api/cmd/pwd", post(handlers::cmd::handle_pwd))
        .route("/api/cmd/cd", post(handlers::cmd::handle_cd))
        .route(
            "/api/cmd/cd_default",
            post(handlers::cmd::handle_cd_default),
        )
        .route("/api/cmd/help", post(handlers::cmd::handle_help))
        .route("/api/deliver", post(handlers::deliver::handle_deliver))
        .route(
            "/api/sessions",
            get(handlers::session::handle_list_sessions),
        )
        .route(
            "/api/sessions",
            post(handlers::session::handle_create_session),
        )
        .route(
            "/api/sessions/{id}/start",
            post(handlers::session::handle_start_session),
        )
        .route(
            "/api/sessions/{id}/messages",
            post(handlers::session::handle_send_message),
        )
        .route(
            "/api/sessions/{id}/permission",
            post(handlers::session::handle_permission),
        )
        .route(
            "/api/sessions/{id}",
            post(handlers::session::handle_stop_session),
        )
        .route(
            "/api/sessions/{id}",
            delete(handlers::session::handle_delete_session),
        )
        .route(
            "/api/sessions/{id}/history",
            get(handlers::session::handle_get_history),
        )
        .route(
            "/api/sessions/{id}/events",
            get(handlers::session::handle_events),
        )
        .route("/api/config", get(handlers::config::handle_get_config))
        .route("/api/config", post(handlers::config::handle_save_config))
        .route(
            "/api/platforms",
            get(handlers::config::handle_get_platforms),
        )
        .route(
            "/api/platforms/require_pairing",
            post(handlers::config::handle_set_require_pairing),
        )
        .route("/api/version", get(handlers::system::handle_version))
        .route(
            "/api/version/check",
            get(handlers::system::handle_update_check),
        )
        .route(
            "/api/update/check",
            get(handlers::system::handle_update_check),
        )
        .route(
            "/api/update",
            get(handlers::system::handle_update_check).post(handlers::system::handle_update),
        )
        .route("/api/restart", post(handlers::system::handle_restart))
        .route(
            "/api/pairing/pending",
            get(handlers::pairing::handle_list_pending),
        )
        .route(
            "/api/pairing/approve",
            post(handlers::pairing::handle_approve),
        )
        .route(
            "/api/pairing/reject",
            post(handlers::pairing::handle_reject),
        )
        .route(
            "/api/pairing/approved",
            get(handlers::pairing::handle_list_approved),
        )
        .route(
            "/api/pairing/approved/set_enabled",
            post(handlers::pairing::handle_set_approval_enabled),
        )
        .route(
            "/api/pairing/approved/remove",
            post(handlers::pairing::handle_delete_approval),
        )
        // WebUI static files
        .route("/", get(handlers::ui::serve_index))
        .route("/{*path}", get(handlers::ui::serve_static))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::ip_allowlist,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware::webui_token_auth,
        ))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
