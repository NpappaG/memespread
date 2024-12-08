use clickhouse::Client;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::services::token::update_token_metrics;
use crate::db::queries::{get_tokens_needing_stats_update, get_tokens_needing_metrics_update};
use tokio::time::Duration;
use futures::stream::StreamExt;

pub async fn start_monitoring(
    db: Client,
    client: Arc<RpcClient>,
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
) {
    tracing::info!("Starting monitoring service...");
    let stats_interval = Duration::from_secs(60);
    let metrics_interval = Duration::from_secs(14400);
    let mut stats_timer = tokio::time::interval(stats_interval);
    let mut metrics_timer = tokio::time::interval(metrics_interval);
    
    let batch_size = 5;
    let max_concurrent_batches = 2;

    loop {
        tokio::select! {
            _ = stats_timer.tick() => {
                tracing::info!("Starting stats monitoring cycle...");
                if let Ok(tokens) = get_tokens_needing_stats_update(&db).await {
                    for token_batch in tokens.chunks(batch_size) {
                        let futures: Vec<_> = token_batch.iter().map(|token| {
                            let client = client.clone();
                            let rate_limiter = rate_limiter.clone();
                            let db = db.clone();
                            let token = token.clone();
                            
                            async move {
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                rate_limiter.until_ready().await;
                                
                                if let Err(e) = update_token_metrics(&client, &rate_limiter, &token, &db).await {
                                    tracing::error!("Failed to update stats for {}: {:?}", token, e);
                                } else {
                                    tracing::info!("Successfully updated stats for {}", token);
                                }
                            }
                        }).collect();

                        futures::stream::iter(futures)
                            .buffer_unordered(max_concurrent_batches)
                            .collect::<Vec<_>>()
                            .await;
                        
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }

            _ = metrics_timer.tick() => {
                tracing::info!("Starting metrics monitoring cycle...");
                if let Ok(tokens) = get_tokens_needing_metrics_update(&db).await {
                    for token_batch in tokens.chunks(batch_size) {
                        let futures: Vec<_> = token_batch.iter().map(|token| {
                            let client = client.clone();
                            let rate_limiter = rate_limiter.clone();
                            let db = db.clone();
                            let token = token.clone();
                            
                            async move {
                                tokio::time::sleep(Duration::from_millis(200)).await;
                                rate_limiter.until_ready().await;
                                
                                if let Err(e) = update_token_metrics(&client, &rate_limiter, &token, &db).await {
                                    tracing::error!("Failed to update metrics for {}: {:?}", token, e);
                                } else {
                                    tracing::info!("Successfully updated metrics for {}", token);
                                }
                            }
                        }).collect();

                        futures::stream::iter(futures)
                            .buffer_unordered(max_concurrent_batches)
                            .collect::<Vec<_>>()
                            .await;
                        
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        }
    }
}