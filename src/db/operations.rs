use anyhow::Result;
use clickhouse::Client;
use crate::types::models::TokenHolderStats;
use solana_sdk::pubkey::Pubkey;

pub async fn insert_token_stats(
    client: &Client,
    mint_address: &str,
    price: f64,
    supply: f64,
    market_cap: f64,
    decimals: u8,
) -> Result<(), anyhow::Error> {
    // Get the timestamp that was just used
    let timestamp: String = client
        .query("SELECT toString(max(timestamp)) FROM token_holders WHERE mint_address = ?")
        .bind(mint_address)
        .fetch_one()
        .await?;

    client
        .query(
            "INSERT INTO token_stats (
                mint_address,
                timestamp,
                price,
                supply,
                market_cap,
                decimals
            ) VALUES (?, toDateTime(?, 'UTC'), ?, ?, ?, ?)"
        )
        .bind(mint_address)
        .bind(&timestamp)
        .bind(price)
        .bind(supply)
        .bind(market_cap)
        .bind(decimals)
        .execute()
        .await?;

    update_monitored_token_timestamp(client, mint_address, &timestamp).await?;

    Ok(())
}

pub async fn insert_token_holders(
    client: &Client,
    mint_address: &str,
    holders: &[(String, u64, Pubkey)],
) -> Result<(), anyhow::Error> {
    tracing::info!("Starting to insert {} holders for {}", holders.len(), mint_address);

    let values = holders.iter()
        .map(|(token_account, amount, holder_address)| 
            format!("('{}', '{}', '{}', {}, now())", 
                mint_address, 
                token_account, 
                holder_address.to_string(), 
                amount
            )
        )
        .collect::<Vec<_>>()
        .join(",");

    let query = format!(
        "INSERT INTO token_holders 
         (mint_address, token_account, holder_address, amount, timestamp) 
         VALUES {}", 
        values
    );

    client.query(&query)
        .execute()
        .await?;

    tracing::info!("Successfully inserted {} holders", holders.len());
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
        "token_stats": {
            "decimals": data.token_stats.decimals,
            "market_cap": (data.token_stats.market_cap * 100.0).round() / 100.0,
            "price": (data.token_stats.price * 1000000000.0).round() / 1000000000.0,
            "supply": (data.token_stats.supply * 100.0).round() / 100.0
        },
        "distribution_stats": {
            "distribution_score": (data.distribution_stats.distribution_score * 10000.0).round() / 10000.0,
            "hhi": (data.distribution_stats.hhi * 10000.0).round() / 10000.0,
            "median_balance": data.distribution_stats.median_balance,
            "mean_balance": data.distribution_stats.mean_balance,
            "total_count": data.distribution_stats.total_count
        },
        "holder_thresholds": data.holder_thresholds.iter().map(|h| {
            serde_json::json!({
                "usd_threshold": h.usd_threshold,
                "holder_count": h.holder_count,
                "total_holders": h.total_holders,
                "pct_total_holders": (h.pct_total_holders * 10000.0).round() / 10000.0,
                "pct_of_10usd": (h.pct_of_10usd * 10000.0).round() / 10000.0,
                "mcap_per_holder": (h.mcap_per_holder * 100.0).round() / 100.0,
                "slice_value_usd": (h.slice_value_usd * 100.0).round() / 100.0
            })
        }).collect::<Vec<_>>(),
        "concentration_metrics": data.concentration_metrics.iter().map(|m| {
            serde_json::json!({
                "top_n": m.top_n,
                "percentage": (m.percentage * 10000.0).round() / 10000.0
            })
        }).collect::<Vec<_>>()
    })
}