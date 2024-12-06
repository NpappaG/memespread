use axum::{
    routing::get,
    Router,
};
use super::handlers::get_token_stats;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use clickhouse::Client;

pub type AppState = (
    Arc<RpcClient>,
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    Client,
);

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/token-stats", get(get_token_stats))
        .with_state(state)
}