use axum::{
    extract::{State, Path},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::db::operations::structure_token_stats;
use crate::services::token::get_token_metrics;
use clickhouse::Client;
use super::error::ApiError;
use crate::services::excluded_accounts::check_new_token_exclusions;

pub type AppState = (
    Arc<RpcClient>,
    Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    Client,
);

#[derive(Deserialize)]
pub struct CreateTokenRequest {
    pub mint_address: String,
}

#[derive(Serialize)]
pub struct CreateTokenResponse {
    status: String,
    message: String,
}

pub async fn create_token_monitor(
    State((_rpc_client, rate_limiter, db)): State<AppState>,
    Json(params): Json<CreateTokenRequest>,
) -> Result<Json<CreateTokenResponse>, ApiError> {
    rate_limiter.until_ready().await;
    
    tracing::info!("Received request to monitor token: {}", params.mint_address);
    
    // Check if token is already monitored
    let is_monitored = db.query(
        "SELECT mint_address FROM monitored_tokens WHERE mint_address = ? LIMIT 1"
    )
        .bind(&params.mint_address)
        .fetch_optional::<String>()
        .await
        .map_err(|e| {
            tracing::error!("Database error checking monitored status: {}", e);
            ApiError::DatabaseError(e.to_string())
        })?;

    if is_monitored.is_some() {
        return Ok(Json(CreateTokenResponse {
            status: "already_monitored".to_string(),
            message: "Token is already being monitored".to_string(),
        }));
    }

    // Add token to monitoring
    db.query(
        "INSERT INTO monitored_tokens (mint_address, last_stats_update, last_metrics_update) 
         VALUES (?, toDateTime('1970-01-01 00:00:00'), toDateTime('1970-01-01 00:00:00'))"
    )
        .bind(&params.mint_address)
        .execute()
        .await
        .map_err(|e| ApiError::DatabaseError(e.to_string()))?;

    // Check for excluded accounts for this new token
    if let Err(e) = check_new_token_exclusions(&_rpc_client, &rate_limiter, &db, &params.mint_address).await {
        tracing::error!("Failed to check excluded accounts for new token: {}", e);
    }

    Ok(Json(CreateTokenResponse {
        status: "monitoring_started".to_string(),
        message: "Token has been added to monitoring. Data will be available soon.".to_string(),
    }))
}

pub async fn get_token_stats(
    State((_rpc_client, rate_limiter, db)): State<AppState>,
    Path(mint_address): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    rate_limiter.until_ready().await;
    
    tracing::info!("Retrieving stats for token: {}", mint_address);
    
    // Check if token is monitored
    let is_monitored = db.query(
        "SELECT mint_address FROM monitored_tokens WHERE mint_address = ? LIMIT 1"
    )
        .bind(&mint_address)
        .fetch_optional::<String>()
        .await
        .map_err(|e| {
            tracing::error!("Database error checking monitored status: {}", e);
            ApiError::DatabaseError(e.to_string())
        })?;

    if is_monitored.is_none() {
        return Err(ApiError::TokenNotMonitored(mint_address));
    }

    match get_token_metrics(&db, &mint_address).await {
        Ok(stats) => {
            tracing::info!("Successfully retrieved stats for {}", mint_address);
            Ok(Json(structure_token_stats(stats)))
        },
        Err(e) => {
            tracing::error!("Error fetching stats: {}", e);
            Err(ApiError::DatabaseError(e.to_string()))
        }
    }
}