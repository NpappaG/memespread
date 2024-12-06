use anyhow::Result;
use clickhouse::Client;
use crate::types::models::TokenHolderStats;
use crate::db::models::TokenStatsRecord;
//use chrono::{DateTime, Utc};
//use serde::Deserialize;

pub async fn insert_token_stats(client: &Client, mint_address: &str, stats: &TokenHolderStats) -> Result<()> {
    // Serialize the arrays to JSON strings
    let holder_thresholds = serde_json::to_string(&stats.holder_thresholds)?;
    let concentration_metrics = serde_json::to_string(&stats.concentration_metrics)?;

    // Insert new stats observation
    client
        .query(
            "INSERT INTO token_stats (
                mint_address,
                price,
                supply,
                market_cap,
                decimals,
                holders,
                holder_thresholds,
                concentration_metrics
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(mint_address)
        .bind(stats.price)
        .bind(stats.supply)
        .bind(stats.market_cap)
        .bind(stats.decimals as u8)
        .bind(stats.holders as u32)
        .bind(holder_thresholds)
        .bind(concentration_metrics)
        .execute()
        .await?;

    // First try to insert if not exists
    client
        .query(
            "INSERT INTO monitored_tokens (
                mint_address,
                last_stats_update,
                last_metrics_update
            ) 
            SELECT 
                ?,
                now(),
                (SELECT coalesce(
                    (SELECT last_metrics_update 
                     FROM monitored_tokens FINAL 
                     WHERE mint_address = ?),
                    toDateTime('1970-01-01 00:00:00')
                ))
            WHERE NOT EXISTS (
                SELECT 1 
                FROM monitored_tokens FINAL 
                WHERE mint_address = ?
            )"
        )
        .bind(mint_address)
        .bind(mint_address)
        .bind(mint_address)
        .execute()
        .await?;

    // Then update if it exists
    client
        .query(
            "ALTER TABLE monitored_tokens 
             UPDATE last_stats_update = now() 
             WHERE mint_address = ?"
        )
        .bind(mint_address)
        .execute()
        .await?;

    Ok(())
}

pub async fn get_latest_token_stats(client: &Client, mint_address: &str) -> Result<Option<TokenStatsRecord>> {
    let result = client
        .query(
            "SELECT 
                mint_address,
                timestamp,
                price,
                supply,
                market_cap,
                CAST(decimals as UInt8) as decimals,
                CAST(holders as UInt32) as holders,
                holder_thresholds,
                concentration_metrics
            FROM token_stats 
            WHERE mint_address = ?
            ORDER BY timestamp DESC
            LIMIT 1"
        )
        .bind(mint_address)
        .fetch_one::<TokenStatsRecord>()
        .await;

    match result {
        Ok(record) => Ok(Some(record)),
        Err(clickhouse::error::Error::RowNotFound) => Ok(None),
        Err(e) => Err(anyhow::anyhow!(e)),
    }
}

pub async fn insert_distribution_metrics(
    client: &Client, 
    mint_address: &str, 
    hhi: f64, 
    distribution_score: f64
) -> Result<()> {
    tracing::debug!(
        "Inserting distribution metrics: mint={}, hhi={}, score={}", 
        mint_address, hhi, distribution_score
    );

    // Insert new distribution metrics observation
    client
        .query(
            "INSERT INTO token_distribution_metrics (
                mint_address,
                hhi,
                distribution_score
            ) VALUES (?, ?, ?)"
        )
        .bind(mint_address)
        .bind(hhi)
        .bind(distribution_score)
        .execute()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to insert distribution metrics: {}", e))?;

    // First try to insert
    client
        .query(
            "INSERT INTO monitored_tokens (
                mint_address,
                last_stats_update,
                last_metrics_update
            ) 
            SELECT 
                ?,
                now(),
                now()
            WHERE NOT EXISTS (
                SELECT 1 
                FROM monitored_tokens FINAL 
                WHERE mint_address = ?
            )"
        )
        .bind(mint_address)
        .bind(mint_address)
        .execute()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to insert into monitored_tokens: {}", e))?;

    // Then update if it exists
    client
        .query(
            "ALTER TABLE monitored_tokens 
             UPDATE last_metrics_update = now() 
             WHERE mint_address = ?"
        )
        .bind(mint_address)
        .execute()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update monitored_tokens: {}", e))?;

    Ok(())
}
