use clickhouse::Client;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::services::token::{get_token_holders, update_token_metrics};
use crate::db::operations::insert_token_stats;
use crate::db::queries::{get_tokens_needing_stats_update, get_tokens_needing_metrics_update};
use tokio::time::Duration;

pub async fn start_monitoring(
    db: Client,
    client: Arc<RpcClient>,
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
) {
    tracing::info!("Starting monitoring service...");
    let stats_interval = Duration::from_secs(60); // 1 minute
    let metrics_interval = Duration::from_secs(14400); // 4 hours
    let mut stats_timer = tokio::time::interval(stats_interval);
    let mut metrics_timer = tokio::time::interval(metrics_interval);

    loop {
        tokio::select! {
            _ = stats_timer.tick() => {
                tracing::info!("Starting stats monitoring cycle...");
                match get_tokens_needing_stats_update(&db).await {
                    Ok(tokens) => {
                        tracing::info!("Found {} tokens needing stats update", tokens.len());
                        for mint_address in tokens {
                            tracing::info!("Processing token: {}", mint_address);
                            match get_token_holders(&client, &rate_limiter, &mint_address, 0.0, &db).await {
                                Ok(stats) => {
                                    match insert_token_stats(&db, &mint_address, &stats).await {
                                        Ok(_) => tracing::info!("Successfully updated stats for {}", mint_address),
                                        Err(e) => tracing::error!("Failed to insert stats for {}: {:?}", mint_address, e),
                                    }
                                }
                                Err(e) => tracing::error!("Failed to get token holders for {}: {:?}", mint_address, e),
                            }
                            // Add small delay between tokens to avoid overwhelming the system
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                    }
                    Err(e) => tracing::error!("Failed to get tokens needing update: {:?}", e),
                }
            }

            _ = metrics_timer.tick() => {
                tracing::info!("Starting metrics monitoring cycle...");
                match get_tokens_needing_metrics_update(&db).await {
                    Ok(tokens) => {
                        tracing::info!("Found {} tokens needing metrics update", tokens.len());
                        for mint_address in tokens {
                            tracing::debug!("Updating metrics for token: {}", mint_address);
                            match update_token_metrics(&client, &rate_limiter, &mint_address, &db).await {
                                Ok(_) => tracing::info!("Successfully updated metrics for {}", mint_address),
                                Err(e) => tracing::error!("Failed to update metrics for {}: {:?}", mint_address, e),
                            }
                        }
                    }
                    Err(e) => tracing::error!("Failed to get tokens needing metrics update: {:?}", e),
                }
            }
        }
    }
}