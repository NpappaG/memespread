use axum::{
    routing::{get, post},
    Router,
};
use super::handlers::{get_token_stats, create_token_monitor};
use super::state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/tokens/:mint_address", get(get_token_stats))
        .route("/tokens", post(create_token_monitor))
        .with_state(state)
}