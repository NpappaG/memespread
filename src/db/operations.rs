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
    client
        .query(
            "INSERT INTO token_stats (
                mint_address,
                price,
                supply,
                market_cap,
                decimals
            ) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(mint_address)
        .bind(price)
        .bind(supply)
        .bind(market_cap)
        .bind(decimals)
        .execute()
        .await?;

    Ok(())
}

pub async fn insert_token_holders(
    client: &Client,
    mint_address: &str,
    holders: &[(String, u64, Pubkey)],
) -> Result<(), anyhow::Error> {
    for (token_account, amount, holder_address) in holders {
        client
            .query(
                "INSERT INTO token_holders (
                    mint_address,
                    token_account,
                    holder_address,
                    amount
                ) VALUES (?, ?, ?, ?)"
            )
            .bind(mint_address)
            .bind(token_account)
            .bind(holder_address.to_string())
            .bind(*amount)
            .execute()
            .await?;
    }

    Ok(())
}

pub async fn update_monitored_token_timestamp(client: &Client, mint_address: &str) -> Result<()> {
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