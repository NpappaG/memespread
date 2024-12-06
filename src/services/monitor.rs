use clickhouse::Client;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use crate::services::token::get_token_holders;
use chrono::Utc;
//use futures::StreamExt;

pub async fn start_monitoring(
    db: Client,
    client: Arc<RpcClient>,
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
) {
    let query = "SELECT mint_address FROM monitored_tokens WHERE last_stats_update < now() - INTERVAL 1 HOUR";
    
    if let Ok(mut cursor) = db.query(query).fetch::<String>() {
        while let Ok(Some(mint_address)) = cursor.next().await {
            if let Ok(stats) = get_token_holders(&client, &rate_limiter, &mint_address, 0.0).await {
                let insert_query = "
                    INSERT INTO token_stats 
                    (timestamp, mint_address, price, supply, market_cap, decimals, holders, 
                     holder_thresholds, concentration_metrics, hhi, distribution_score)
                    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                ";
                
                if let Err(e) = db.query(insert_query)
                    .bind(Utc::now())
                    .bind(&mint_address)
                    .bind(stats.price)
                    .bind(stats.supply)
                    .bind(stats.market_cap)
                    .bind(stats.decimals)
                    .bind(stats.holders as u32)
                    .bind(serde_json::to_string(&stats.holder_thresholds).unwrap())
                    .bind(serde_json::to_string(&stats.concentration_metrics).unwrap())
                    .bind(stats.hhi)
                    .bind(stats.distribution_score)
                    .execute()
                    .await 
                {
                    tracing::error!("Failed to insert stats for {}: {:?}", mint_address, e);
                }
            }
        }
    }
}