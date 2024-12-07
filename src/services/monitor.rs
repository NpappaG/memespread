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
                if let Ok(tokens) = get_tokens_needing_stats_update(&db).await {
                    for token in tokens {
                        rate_limiter.until_ready().await;
                        tracing::info!("Processing token: {}", token);
                        match get_token_holders(&client, &rate_limiter, &token, 0.0, &db).await {
                            Ok(stats) => {
                                match insert_token_stats(&db, &token, &stats).await {
                                    Ok(_) => tracing::info!("Successfully updated stats for {}", token),
                                    Err(e) => tracing::error!("Failed to insert stats for {}: {:?}", token, e),
                                }
                            }
                            Err(e) => tracing::error!("Failed to get token holders for {}: {:?}", token, e),
                        }
                        // Add small delay between tokens to avoid overwhelming the system
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }

            _ = metrics_timer.tick() => {
                tracing::info!("Starting metrics monitoring cycle...");
                if let Ok(tokens) = get_tokens_needing_metrics_update(&db).await {
                    for token in tokens {
                        rate_limiter.until_ready().await;
                        tracing::debug!("Updating metrics for token: {}", token);
                        match update_token_metrics(&client, &rate_limiter, &token, &db).await {
                            Ok(_) => tracing::info!("Successfully updated metrics for {}", token),
                            Err(e) => tracing::error!("Failed to update metrics for {}: {:?}", token, e),
                        }
                    }
                }
            }
        }
    }
}