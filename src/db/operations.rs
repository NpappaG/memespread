use anyhow::Result;
use clickhouse::Client;
use crate::types::models::TokenHolderStats;
use crate::db::models::{TokenStatsRecord, TokenHolderThresholdRecord, TokenConcentrationMetricRecord, TokenDistributionMetricRecord};

pub async fn insert_token_stats(client: &Client, mint_address: &str, stats: &TokenHolderStats) -> Result<()> {
    // Insert base stats
    client
        .query(
            "INSERT INTO token_stats (
                mint_address,
                timestamp,
                price,
                supply,
                market_cap,
                decimals,
                holders
            ) VALUES (?, toDateTime(now(), 'UTC'), ?, ?, ?, ?, ?)"
        )
        .bind(mint_address)
        .bind(stats.price)
        .bind(stats.supply)
        .bind(stats.market_cap)
        .bind(stats.decimals as u8)
        .bind(stats.holders as u32)
        .execute()
        .await?;

    // Insert holder thresholds
    for threshold in &stats.holder_thresholds {
        client
            .query(
                "INSERT INTO token_holder_thresholds (
                    mint_address,
                    usd_threshold,
                    holder_count,
                    percentage,
                    percentage_of_10
                ) VALUES (?, ?, ?, ?, ?)"
            )
            .bind(mint_address)
            .bind(threshold.usd_threshold)
            .bind(threshold.count as u32)
            .bind(threshold.percentage)
            .bind(threshold.percentage_of_10)
            .execute()
            .await?;
    }

    // Insert concentration metrics
    for metric in &stats.concentration_metrics {
        client
            .query(
                "INSERT INTO token_concentration_metrics (
                    mint_address,
                    top_n,
                    percentage
                ) VALUES (?, ?, ?)"
            )
            .bind(mint_address)
            .bind(metric.top_n as u32)
            .bind(metric.percentage)
            .execute()
            .await?;
    }

    update_monitored_token(client, mint_address).await?;

    Ok(())
}

pub async fn get_latest_token_stats(client: &Client, mint_address: &str) -> Result<Option<TokenHolderStats>> {
    tracing::debug!("Starting get_latest_token_stats for {}", mint_address);
    
    let query = "SELECT 
        mint_address,
        timestamp,
        price,
        supply,
        market_cap,
        decimals,
        holders
    FROM token_stats 
    WHERE mint_address = ?
    ORDER BY timestamp DESC 
    LIMIT 1";
    
    tracing::debug!("Executing query: {}", query);
    
    let base_stats = client
        .query(query)
        .bind(mint_address)
        .fetch_all::<TokenStatsRecord>()
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch base stats: {:?}", e);
            e
        })?;
        
    let base_stats = match base_stats.first() {
        Some(stats) => stats,
        None => return Ok(None),
    };

    tracing::debug!("Received base stats: {:?}", base_stats);

    tracing::debug!("Fetching holder thresholds");
    // Get holder thresholds
    let holder_thresholds = client
        .query(
            "SELECT 
                toString(mint_address) as mint_address,
                timestamp,
                usd_threshold,
                holder_count,
                percentage,
                percentage_of_10
            FROM token_holder_thresholds
            WHERE mint_address = ?
            ORDER BY timestamp DESC, usd_threshold
            LIMIT 6"
        )
        .bind(mint_address)
        .fetch_all::<TokenHolderThresholdRecord>()
        .await?;

    tracing::debug!("Found {} holder thresholds", holder_thresholds.len());

    // Get concentration metrics - removed timestamp constraint
    let concentration_metrics = client
        .query(
            "SELECT 
                toString(mint_address) as mint_address,
                timestamp,
                top_n,
                percentage
            FROM token_concentration_metrics
            WHERE mint_address = ?
            ORDER BY timestamp DESC, top_n
            LIMIT 6"
        )
        .bind(mint_address)
        .fetch_all::<TokenConcentrationMetricRecord>()
        .await?;

    // Get latest distribution metrics
    let distribution = client
        .query(
            "SELECT 
                toString(mint_address) as mint_address,
                timestamp,
                hhi,
                distribution_score
            FROM token_distribution_metrics
            WHERE mint_address = ?
            ORDER BY timestamp DESC
            LIMIT 1"
        )
        .bind(mint_address)
        .fetch_one::<TokenDistributionMetricRecord>()
        .await;

    let (hhi, distribution_score) = match distribution {
        Ok(record) => (record.hhi, record.distribution_score),
        Err(e) => {
            tracing::error!("Error fetching distribution metrics: {}", e);
            (0.0, 0.0)
        }
    };

    // Convert to TokenHolderStats
    Ok(Some(TokenHolderStats {
        price: base_stats.price,
        supply: base_stats.supply,
        market_cap: base_stats.market_cap,
        decimals: base_stats.decimals,
        holders: base_stats.holders as usize,
        raw_holders: None,
        holder_thresholds: holder_thresholds.into_iter().map(|h| crate::types::models::HolderThreshold {
            usd_threshold: h.usd_threshold,
            count: h.holder_count as i32,
            percentage: h.percentage,
            percentage_of_10: h.percentage_of_10,
        }).collect(),
        concentration_metrics: concentration_metrics.into_iter().map(|c| crate::types::models::ConcentrationMetric {
            top_n: c.top_n as i32,
            percentage: c.percentage,
        }).collect(),
        hhi,
        distribution_score,
    }))
}

async fn update_monitored_token(client: &Client, mint_address: &str) -> Result<()> {
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
        .await?;

    client
        .query(
            "ALTER TABLE monitored_tokens 
             UPDATE last_metrics_update = now() 
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
                "percentage": (h.percentage * 10000.0).round() / 10000.0,
                "percentage_of_10": (h.percentage_of_10 * 10000.0).round() / 10000.0
            })
        }).collect::<Vec<_>>(),
        "decimals": data.decimals,
        "distribution_score": (data.distribution_score * 10000.0).round() / 10000.0,
        "hhi": (data.hhi * 10000.0).round() / 10000.0,
        "holders": data.holders,
        "market_cap": (data.market_cap * 100.0).round() / 100.0,
        "price": (data.price * 1000000000.0).round() / 1000000000.0,
        "supply": (data.supply * 100.0).round() / 100.0
    })
}
