use axum::{
    extract::{State, Query},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::services::token::get_token_holders;
use crate::db::operations::{get_latest_token_stats, insert_token_stats};
use clickhouse::Client;
use super::error::ApiError;

pub type AppState = (
    Arc<RpcClient>,
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    Client,
);

#[derive(Deserialize)]
pub struct TokenParams {
    pub mint_address: String,
}

pub async fn get_token_stats(
    State((rpc_client, rate_limiter, db)): State<AppState>,
    Query(params): Query<TokenParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // First try to get cached stats
    if let Ok(Some(cached_stats)) = get_latest_token_stats(&db, &params.mint_address).await {
        return Ok(Json(serde_json::to_value(cached_stats).unwrap()));
    }
    
    // If no cached stats, fetch new ones
    match get_token_holders(
        &rpc_client,
        &rate_limiter,
        &params.mint_address,
        0.0,
        &db
    ).await {
        Ok(stats) => {
            // Save the stats before returning
            if let Err(e) = insert_token_stats(&db, &params.mint_address, &stats).await {
                return Err(ApiError::DatabaseError(e.to_string()));
            }
            Ok(Json(serde_json::to_value(stats).unwrap()))
        },
        Err(e) => Err(ApiError::RpcError(e.to_string())),
    }
}

