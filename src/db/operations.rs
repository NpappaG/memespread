use anyhow::Result;
use clickhouse::Client;
use crate::types::models::TokenHolderStats;
use solana_sdk::pubkey::Pubkey;

use chrono::{DateTime, Utc};

pub async fn insert_token_stats(
    client: &Client,
    mint_address: &str,
    price: f64,
    supply: f64,
    market_cap: f64,
    decimals: u8,
    _timestamp: Option<DateTime<Utc>>,
) -> Result<String, anyhow::Error> {
    client
        .query(
            "INSERT INTO token_stats (
                mint_address,
                timestamp,
                price,
                supply,
                market_cap,
                decimals
            ) VALUES (?, now('UTC'), ?, ?, ?, ?)"
        )
        .bind(mint_address)
        .bind(price)
        .bind(supply)
        .bind(market_cap)
        .bind(decimals)
        .execute()
        .await?;

    let timestamp: String = client
        .query("SELECT toString(timestamp) FROM token_stats WHERE mint_address = ? ORDER BY timestamp DESC LIMIT 1")
        .bind(mint_address)
        .fetch_one()
        .await?;

    Ok(timestamp)
}

pub async fn insert_token_holders(
    client: &Client,
    mint_address: &str,
    holders: &[(String, u64, Pubkey)],
    timestamp: &str,
) -> Result<(), anyhow::Error> {
    // Create one large values string for all holders
    let values = holders.iter().enumerate()
        .map(|(i, _)| {
            if i == 0 { " (?, ?, ?, ?, ?)" } else { ",(?, ?, ?, ?, ?)" }
        })
        .collect::<String>();

    let mut query = client.query(&format!(
        "INSERT INTO token_holders (mint_address, token_account, holder_address, amount, timestamp) VALUES{}",
        values
    ));

    // Bind all values in one go
    for (token_account, amount, holder_address) in holders {
        query = query
            .bind(mint_address)
            .bind(token_account)
            .bind(holder_address.to_string())
            .bind(*amount)
            .bind(timestamp);
    }

    query.execute().await?;

    Ok(())
}

pub async fn update_monitored_token_timestamp(
    client: &Client, 
    mint_address: &str,
    timestamp: &str,
) -> Result<()> {
    client
        .query(
            "ALTER TABLE monitored_tokens 
             UPDATE last_stats_update = toDateTime(?, 'UTC'), 
                    last_metrics_update = toDateTime(?, 'UTC')
             WHERE mint_address = ?"
        )
        .bind(timestamp)
        .bind(timestamp)
        .bind(mint_address)
        .execute()
        .await?;

    Ok(())
}

pub fn structure_token_stats(data: TokenHolderStats) -> serde_json::Value {
    serde_json::json!({
        "concentration_metrics": data.concentration_metrics.iter().map(|m| {
            serde_json::json!({
                "top_n": m.top_n,
                "percentage": (m.percentage * 10000.0).round() / 10000.0
            })
        }).collect::<Vec<_>>(),
        "holder_thresholds": data.holder_thresholds.iter().map(|h| {
            serde_json::json!({
                "usd_threshold": h.usd_threshold,
                "count": h.count,
                "percentage": (h.percentage * 10000.0).round() / 10000.0
            })
        }).collect::<Vec<_>>(),
        "decimals": data.decimals,
        "distribution_score": (data.distribution_score * 10000.0).round() / 10000.0,
        "hhi": (data.hhi * 10000.0).round() / 10000.0,
        "market_cap": (data.market_cap * 100.0).round() / 100.0,
        "price": (data.price * 1000000000.0).round() / 1000000000.0,
        "supply": (data.supply * 100.0).round() / 100.0
    })
}