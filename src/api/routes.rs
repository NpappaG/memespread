use axum::{
    routing::{get, post},
    Router,
};
use super::handlers::{get_token_stats, create_token_monitor, get_all_tokens};
use super::state::AppState;
use tower_http::cors::{CorsLayer, Any};

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/tokens/:mint_address", get(get_token_stats))
        .route("/tokens", get(get_all_tokens))
        .route("/tokens", post(create_token_monitor))
        .layer(cors)
        .with_state(state)
}