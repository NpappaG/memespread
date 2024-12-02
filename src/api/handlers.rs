use axum::{
  extract::{Query, State},
  Json,
  http::StatusCode,
};
use std::sync::Arc;
use serde_json::json;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};

use crate::types::models::{TokenQuery, TokenHolderStats};
use crate::services::token::get_token_holders;

pub async fn token_stats(
  Query(params): Query<TokenQuery>,
  State((rpc_client, rate_limiter)): State<(Arc<RpcClient>, Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>)>,
) -> Result<Json<TokenHolderStats>, (StatusCode, Json<serde_json::Value>)> {
    // TODO: Fetch price from Jupiter API
    let price_in_usd = 0.0; // Placeholder until Jupiter integration

    match get_token_holders(&rpc_client, &rate_limiter, &params.mint_address, price_in_usd).await {
        Ok(stats) => Ok(Json(stats)),
        Err(e) => {
            Err((StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": format!("Failed to get token holders: {}", e)
            }))))
        }
    }
}

