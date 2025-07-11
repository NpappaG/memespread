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
    
    let mut stats_running = false;
    let mut metrics_running = false;

    loop {
        tokio::select! {
            _ = stats_timer.tick(), if !stats_running => {
                process_stats(&mut stats_running, &db, &client, &rate_limiter).await;
            }

            _ = metrics_timer.tick(), if !metrics_running => {
                process_metrics(&mut metrics_running, &db, &client, &rate_limiter).await;
            }
        }
    }
}

async fn process_stats(stats_running: &mut bool, db: &Client, client: &Arc<RpcClient>, rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>) {
    *stats_running = true;
    tracing::info!("Starting stats monitoring cycle...");
    
    let batch_size = 1;

    match get_tokens_needing_stats_update(&db).await {
        Ok(tokens) => {
            tracing::info!("Found {} tokens needing stats update", tokens.len());
            for token_batch in tokens.chunks(batch_size) {
                let futures: Vec<_> = token_batch.iter().map(|token| {
                    let client = client.clone();
                    let rate_limiter = rate_limiter.clone();
                    let db = db.clone();
                    let token = token.clone();
                    
                    async move {
                        tracing::debug!("Processing stats for token {}", token);
                        rate_limiter.until_ready().await;
                        
                        match update_token_metrics(&client, &rate_limiter, &token, &db).await {
                            Ok(_) => {
                                if let Err(e) = db.query(
                                    "ALTER TABLE monitored_tokens UPDATE last_stats_update = now() WHERE mint_address = ?"
                                )
                                    .bind(&token)
                                    .execute()
                                    .await 
                                {
                                    tracing::error!("Failed to update last_stats_update for {}: {:?}", token, e);
                                } else {
                                    tracing::info!("Successfully updated stats and timestamp for {}", token);
                                }
                            }
                            Err(e) => tracing::error!("Failed to update stats for {}: {:?}", token, e),
                        }
                    }
                }).collect();

                futures::future::join_all(futures).await;
            }
        }
        Err(e) => tracing::error!("Failed to get tokens needing stats update: {:?}", e),
    }

    *stats_running = false;
}

async fn process_metrics(metrics_running: &mut bool, db: &Client, client: &Arc<RpcClient>, rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>) {
    *metrics_running = true;
    tracing::info!("Starting metrics monitoring cycle...");
    
    let batch_size = 5;
    let max_concurrent_batches = 2;

    if let Ok(tokens) = get_tokens_needing_metrics_update(db).await {
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
    
    *metrics_running = false;
}