use axum::{
    extract::{State, Query},
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::db::operations::{get_latest_token_stats, structure_token_stats};
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
    State((_rpc_client, rate_limiter, db)): State<AppState>,
    Query(params): Query<TokenParams>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Wait for rate limiter before proceeding
    rate_limiter.until_ready().await;
    
    tracing::info!("Received request for token: {}", params.mint_address);
    
    // First check if token is monitored
    let is_monitored = db.query(
        "SELECT mint_address 
         FROM monitored_tokens 
         WHERE mint_address = ? 
         LIMIT 1"
    )
        .bind(&params.mint_address)
        .fetch_optional::<String>()
        .await
        .map_err(|e| {
            tracing::error!("Database error checking monitored status: {}", e);
            ApiError::DatabaseError(e.to_string())
        })?;

    tracing::info!("Token {} monitored status: {:?}", params.mint_address, is_monitored);

    if is_monitored.is_none() {
        // Instead of immediately fetching data, just add to monitoring
        tracing::info!("Token {} not monitored, adding to monitoring", params.mint_address);
        
        // Add to monitored_tokens
        db.query(
            "INSERT INTO monitored_tokens (mint_address, last_stats_update, last_metrics_update) 
             VALUES (?, toDateTime('1970-01-01 00:00:00'), toDateTime('1970-01-01 00:00:00'))"
        )
            .bind(&params.mint_address)
            .execute()
            .await
            .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

        return Ok(Json(serde_json::json!({
            "status": "monitoring_started",
            "message": "Token has been added to monitoring. Data will be available soon."
        })));
    }

    // Get stats for monitored token
    tracing::info!("Fetching stats for monitored token: {}", params.mint_address);
    match get_latest_token_stats(&db, &params.mint_address).await {
        Ok(Some(stats)) => {
            tracing::info!("Successfully retrieved stats for {}", params.mint_address);
            Ok(Json(structure_token_stats(stats)))
        },
        Ok(None) => {
            tracing::info!("No stats available for monitored token: {}", params.mint_address);
            Ok(Json(serde_json::json!({
                "status": "no_data",
                "message": "Token is monitored but no data is available yet."
            })))
        },
        Err(e) => {
            tracing::error!("Error fetching stats: {}", e);
            Err(ApiError::DatabaseError(e.to_string()))
        }
    }
}

