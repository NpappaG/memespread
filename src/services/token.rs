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
use crate::types::models::{TokenHolderStats, HolderThreshold, ConcentrationMetric};
use serde_json::Value;
use tracing::info;
use clickhouse::Client;
use solana_account_decoder::UiAccountEncoding;
use crate::db::models::{TokenHolderThresholdRecord, TokenConcentrationMetricRecord, TokenStatsRecord};
use crate::db::operations::{insert_token_stats, insert_token_holders};
use chrono::{Utc, TimeZone};


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


async fn get_token_price(mint_address: &str) -> Result<f64, anyhow::Error> {
    let url = format!("https://api.jup.ag/price/v2?ids={}", mint_address);
    let response = reqwest::get(url).await?;
    let json: Value = response.json().await?;
    
    info!("Jupiter API response: {:?}", json);
    
    let price = json["data"][mint_address]["price"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse price"))?
        .parse::<f64>()?;
    
    Ok(price)
}

pub async fn get_token_metrics(
    clickhouse_client: &Client,
    mint_address: &str,
) -> Result<TokenHolderStats, anyhow::Error> {
    // Single query to get latest timestamp for this token's metrics
    let latest_ts = clickhouse_client
        .query("SELECT max(timestamp) as ts FROM token_distribution WHERE mint_address = ?")
        .bind(mint_address)
        .fetch_one::<i64>()
        .await?;

    // Convert i64 to DateTime<Utc>
    let latest_ts_datetime = Utc.timestamp_opt(latest_ts, 0).unwrap();

    // Get all metrics using the same timestamp
    let (base_stats, holder_count, thresholds, concentration, distribution) = tokio::try_join!(
        clickhouse_client
            .query("SELECT price, supply, market_cap, decimals FROM token_stats WHERE mint_address = ? AND timestamp = ?")
            .bind(mint_address)
            .bind(latest_ts_datetime)
            .fetch_one::<TokenStatsRecord>(),
        
        clickhouse_client
            .query("SELECT count() FROM token_holder_balances WHERE mint_address = ? AND timestamp = ?")
            .bind(mint_address)
            .bind(latest_ts_datetime)
            .fetch_one::<u32>(),
            
        clickhouse_client
            .query("SELECT usd_threshold, holder_count, percentage FROM token_holder_counts WHERE mint_address = ? AND timestamp = ?")
            .bind(mint_address)
            .bind(format!("{} UTC", latest_ts_datetime.format("%Y-%m-%d %H:%M:%S")))
            .fetch_all::<TokenHolderThresholdRecord>(),
            
        clickhouse_client
            .query("SELECT top_n, percentage FROM token_concentration WHERE mint_address = ? AND timestamp = ?")
            .bind(mint_address)
            .bind(format!("{} UTC", latest_ts_datetime.format("%Y-%m-%d %H:%M:%S")))
            .fetch_all::<TokenConcentrationMetricRecord>(),
            
        clickhouse_client
            .query("SELECT hhi, distribution_score FROM token_distribution_metrics WHERE mint_address = ? AND timestamp = ?")
            .bind(mint_address)
            .bind(format!("{} UTC", latest_ts_datetime.format("%Y-%m-%d %H:%M:%S")))
            .fetch_one::<(f64, f64)>()
    )?;

    Ok(TokenHolderStats {
        mint_address: mint_address.to_string(),
        price: base_stats.price,
        supply: base_stats.supply,
        market_cap: base_stats.market_cap,
        decimals: base_stats.decimals,
        total_count: holder_count as usize,
        holder_thresholds: thresholds.into_iter().map(|t| HolderThreshold {
            usd_threshold: t.usd_threshold,
            count: t.holder_count as u64,
            percentage: t.percentage,
        }).collect(),
        concentration_metrics: concentration.into_iter().map(|m| ConcentrationMetric {
            top_n: m.top_n as i32,
            percentage: m.percentage,
        }).collect(),
        hhi: distribution.0,
        distribution_score: distribution.1,
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
