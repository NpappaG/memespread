use clickhouse::Client;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::services::token::get_token_holders;
use crate::db::operations::insert_token_stats;
use crate::db::queries::get_tokens_needing_stats_update;
use tokio::time::{sleep, Duration};

pub async fn start_monitoring(
    db: Client,
    client: Arc<RpcClient>,
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
) {
    loop {
        tracing::info!("Starting monitoring cycle...");
        
        // Monitor token stats (every hour)
        match get_tokens_needing_stats_update(&db).await {
            Ok(tokens) => {
                tracing::info!("Found {} tokens needing stats update", tokens.len());
                for mint_address in tokens {
                    tracing::debug!("Updating stats for token: {}", mint_address);
                    match get_token_holders(&client, &rate_limiter, &mint_address, 0.0, &db).await {
                        Ok(stats) => {
                            match insert_token_stats(&db, &mint_address, &stats).await {
                                Ok(_) => tracing::info!("Successfully updated stats for {}", mint_address),
                                Err(e) => tracing::error!("Failed to insert stats for {}: {:?}", mint_address, e),
                            }
                        }
                        Err(e) => tracing::error!("Failed to get token holders for {}: {:?}", mint_address, e),
                    }
                }
            }
            Err(e) => tracing::error!("Failed to get tokens needing update: {:?}", e),
        }

        // Sleep for 1 minute before next check
        sleep(Duration::from_secs(60)).await;
        tracing::debug!("Monitor cycle complete, sleeping for 60 seconds");
    }
}