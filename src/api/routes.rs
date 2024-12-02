use axum::{Router, routing::get};
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use tower_http::cors::CorsLayer;
use super::handlers::token_stats;
use axum::http::{Method, HeaderValue};
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};

pub type AppState = (
    Arc<RpcClient>,
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>
);

pub fn create_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin("*".parse::<HeaderValue>().unwrap())
        .allow_methods([Method::GET]);

    Router::new()
        .route("/token-stats", get(token_stats))
        .layer(cors)
        .with_state(state)
}