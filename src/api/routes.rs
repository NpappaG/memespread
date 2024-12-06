use axum::{
    routing::get,
    Router,
};
use super::handlers::get_token_stats;
use super::state::AppState;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/token-stats", get(get_token_stats))
        .with_state(state)
}