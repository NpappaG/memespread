use anyhow::Result;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_filter::{Memcmp, MemcmpEncodedBytes},
};
use solana_sdk::{
    program_pack::Pack,
    pubkey::Pubkey,
};
use spl_token::state::Account as TokenAccount;
use std::sync::Arc;
use std::str::FromStr;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use rayon::prelude::*;
use crate::types::models::{TokenHolderStats, HolderThreshold, ConcentrationMetric, TokenStats, DistributionStats};
use serde_json::Value;
use tracing::info;
use clickhouse::Client;
use solana_account_decoder::UiAccountEncoding;
use crate::db::models::{TokenStatsRecord, TokenHolderThresholdRecord, TokenConcentrationMetricRecord, TokenDistributionMetricRecord};
use crate::db::operations::{insert_token_stats, insert_token_holders};


async fn fetch_and_sort_holders(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    mint_pubkey: &Pubkey,
    min_balance: u64,
) -> Result<Vec<(String, u64, Pubkey)>, anyhow::Error> {
    rate_limiter.until_ready().await;
    
    let config = solana_client::rpc_config::RpcProgramAccountsConfig {
        filters: Some(vec![
            solana_client::rpc_filter::RpcFilterType::Memcmp(Memcmp::new(
                0,
                MemcmpEncodedBytes::Base58(mint_pubkey.to_string()),
            )),
            solana_client::rpc_filter::RpcFilterType::DataSize(TokenAccount::LEN as u64),
        ]),
        account_config: solana_client::rpc_config::RpcAccountInfoConfig {
            encoding: Some(UiAccountEncoding::Base64),
            ..Default::default()
        },
        with_context: None,
    };

    let accounts = client.get_program_accounts_with_config(&spl_token::ID, config).await?;
    info!("Found {} total token accounts", accounts.len());

    let mut holders = accounts
        .into_par_iter()
        .filter_map(|(pubkey, account)| {
            TokenAccount::unpack(&account.data).ok()
                .filter(|token_account| {
                    token_account.amount > min_balance && 
                    token_account.state == spl_token::state::AccountState::Initialized
                })
                .map(|token_account| (pubkey.to_string(), token_account.amount, token_account.owner))
        })
        .collect::<Vec<_>>();
    
    holders.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(holders)
}


pub async fn get_token_price(mint_address: &str) -> Result<f64, anyhow::Error> {
    let url = format!("https://lite-api.jup.ag/price/v3?ids={}", mint_address);
    let response = reqwest::get(url).await?;
    let json: Value = response.json().await?;
    
    info!("Jupiter API response: {:?}", json);
    
    // The new API format returns data directly without a "data" wrapper
    // and uses "usdPrice" instead of "price"
    if let Some(token_data) = json.get(mint_address) {
        if let Some(price) = token_data["usdPrice"].as_f64() {
            return Ok(price);
        }
    }
    
    Err(anyhow::anyhow!("Failed to parse price from Jupiter API response"))
}

pub async fn get_token_metrics(
    clickhouse_client: &Client,
    mint_address: &str,
) -> Result<TokenHolderStats, anyhow::Error> {
    // Get token stats
    let stats: TokenStatsRecord = clickhouse_client
        .query("SELECT price, supply, market_cap, decimals FROM token_stats WHERE mint_address = ? ORDER BY timestamp DESC LIMIT 1")
        .bind(mint_address)
        .fetch_one()
        .await?;

    // Get distribution metrics
    let distribution: Option<TokenDistributionMetricRecord> = clickhouse_client
        .query("SELECT mint_address, timestamp, hhi, distribution_score FROM token_distribution WHERE mint_address = ? ORDER BY timestamp DESC LIMIT 1")
        .bind(mint_address)
        .fetch_optional()
        .await?;

    // Get holder thresholds
    info!("Fetching holder thresholds for mint: {}", mint_address);
    let thresholds: Vec<TokenHolderThresholdRecord> = clickhouse_client
        .query("
            SELECT 
                mint_address,
                timestamp,
                usd_threshold,
                holder_count,
                total_holders,
                pct_total_holders,
                pct_of_10usd,
                mcap_per_holder,
                slice_value_usd
            FROM token_holder_counts 
            WHERE mint_address = ? 
            ORDER BY timestamp DESC, usd_threshold ASC
            LIMIT 5
        ")
        .bind(mint_address)
        .fetch_all()
        .await?;

    // Get concentration metrics - fetch all records for the latest timestamp
    let concentration: Vec<TokenConcentrationMetricRecord> = clickhouse_client
        .query("
            SELECT 
                mint_address,
                timestamp,
                top_n,
                percentage
            FROM token_concentration 
            WHERE mint_address = ? 
            ORDER BY timestamp DESC, top_n ASC
            LIMIT 6
        ")
        .bind(mint_address)
        .fetch_all()
        .await?;

    // Add debug logging
    tracing::info!("Found {} concentration metrics: {:?}", concentration.len(), 
        concentration.iter().map(|c| c.top_n).collect::<Vec<_>>());

    tracing::info!("Thresholds count: {}", thresholds.len());
    tracing::info!("First threshold: {:?}", thresholds.first());

    let holder_thresholds = thresholds.into_iter().map(|t| HolderThreshold {
        usd_threshold: t.usd_threshold,
        holder_count: t.holder_count as u64,
        total_holders: t.total_holders as u64,
        pct_total_holders: t.pct_total_holders,
        pct_of_10usd: t.pct_of_10usd,
        mcap_per_holder: t.mcap_per_holder,
        slice_value_usd: t.slice_value_usd
    }).collect();

    Ok(TokenHolderStats {
        mint_address: mint_address.to_string(),
        token_stats: TokenStats {
            price: stats.price,
            supply: stats.supply,
            market_cap: stats.market_cap,
            decimals: stats.decimals,
        },
        distribution_stats: DistributionStats {
            total_count: 0,
            hhi: distribution.as_ref().map_or(0.0, |d| d.hhi),
            distribution_score: distribution.as_ref().map_or(0.0, |d| d.distribution_score),
            median_balance: 0.0,
            mean_balance: 0.0,
        },
        holder_thresholds,
        concentration_metrics: concentration.into_iter().map(|c| ConcentrationMetric {
            top_n: c.top_n as i32,
            percentage: c.percentage,
        }).collect(),
    })
}

pub async fn update_token_metrics(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    mint_address: &str,
    clickhouse_client: &Client,
) -> Result<()> {
    // Fetch holders first
    let mint_pubkey = Pubkey::from_str(mint_address)?;
    let mint_account = client.get_account(&mint_pubkey).await?;
    let mint_data = spl_token::state::Mint::unpack(&mint_account.data)?;

    let holders = fetch_and_sort_holders(client, rate_limiter, &mint_pubkey, 1).await?;
    
    // Insert holders first
    insert_token_holders(clickhouse_client, mint_address, &holders).await?;

    // Get price and other metrics
    let price = get_token_price(mint_address).await?;
    let market_cap = price * mint_data.supply as f64;

    // Insert stats (it will use the timestamp from holders)
    insert_token_stats(
        clickhouse_client,
        mint_address,
        price,
        mint_data.supply as f64,
        market_cap,
        mint_data.decimals as u8,
    ).await?;

    Ok(())
}
