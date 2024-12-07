use anyhow::Result;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_filter::{Memcmp, MemcmpEncodedBytes},
};
use solana_sdk::{
    program_pack::Pack,
    pubkey::Pubkey,
};
use solana_account_decoder::UiAccountEncoding;
use spl_token::state::{Account as TokenAccount, Mint};
use std::sync::Arc;
use std::str::FromStr;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use std::collections::HashSet;
use rayon::prelude::*;
use futures;
use crate::types::models::{TokenHolderStats, HolderThreshold, ConcentrationMetric};
use serde_json::Value;
use super::excluded_accounts::{PROGRAM_IDS, EXCLUDED_OWNERS};
use crate::db::operations::insert_distribution_metrics;
use tracing::info;
use clickhouse::Client;

async fn fetch_and_sort_holders(
    client: &Arc<RpcClient>,
    mint_pubkey: &Pubkey,
    min_balance: u64,
) -> Result<(Vec<(String, u64, Pubkey)>, usize), anyhow::Error> {
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

    let mut holders: Vec<(String, u64, Pubkey)> = accounts
        .into_par_iter()
        .filter_map(|(pubkey, account)| {
            TokenAccount::unpack(&account.data).ok()
                .filter(|token_account| {
                    token_account.amount > min_balance && 
                    token_account.state == spl_token::state::AccountState::Initialized
                })
                .map(|token_account| (pubkey.to_string(), token_account.amount, token_account.owner))
        })
        .collect();
    
    let holder_count = holders.len();
    info!("Found {} accounts with balance > {}", holder_count, min_balance);

    let unique_owners: HashSet<_> = holders.iter().map(|(_, _, owner)| owner).collect();
    info!("Found {} unique owners with balance > {}", unique_owners.len(), min_balance);
    
    holders.sort_by(|a, b| b.1.cmp(&a.1));
    Ok((holders, holder_count))
}

async fn identify_program_owned_accounts(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    holders: &[(String, u64, Pubkey)],
    mint_supply: u64,
    market_cap: f64,
) -> Result<HashSet<Pubkey>, anyhow::Error> {
    let program_ids: HashSet<&str> = PROGRAM_IDS.iter().copied().collect();
    let excluded_owners: HashSet<&str> = EXCLUDED_OWNERS.iter().copied().collect();

    // Dynamic threshold based on market cap (in USD)
    let threshold = match market_cap {
        mc if mc > 1_000_000_000.0 => 0.005,  // 0.5% for >$1B market cap
        mc if mc > 100_000_000.0 => 0.003,    // 0.2% for >$100M market cap
        mc if mc > 10_000_000.0 => 0.002,     // 0.1% for >$10M market cap
        mc if mc > 5_000_000.0 => 0.001,     // 0.1% for >$10M market cap
        _ => 0.0003                         // 0.05% for smaller market caps
    };

    info!("Using threshold of {}% based on ${:.2}M market cap", 
        threshold * 100.0, 
        market_cap / 1_000_000.0);

    let large_holders: Vec<(Pubkey, f64)> = holders.iter()
        .filter_map(|(_, amount, owner)| {
            let percentage = (*amount as f64) / (mint_supply as f64);
            if percentage >= threshold {
                Some((*owner, percentage))
            } else {
                None
            }
        })
        .collect();

    info!("Found {} holders above {}%", large_holders.len(), threshold * 100.0);

    let mut excluded_accounts = HashSet::new();
    let chunk_size = 25;

    // First, add known excluded owners
    for (owner, percentage) in &large_holders {
        let owner_str = owner.to_string();
        if excluded_owners.contains(owner_str.as_str()) {
            info!("Excluded known LP {} holding {:.2}% of supply", 
                owner_str,
                percentage * 100.0);
            excluded_accounts.insert(*owner);
        }
    }

    // Then check for program-owned accounts
    for (batch_num, chunks) in large_holders.chunks(chunk_size * 4).enumerate() {
        let futures: Vec<_> = chunks.chunks(chunk_size)
            .map(|chunk| {
                let pubkeys = chunk.iter()
                    .map(|(owner, _)| *owner)
                    .collect::<Vec<Pubkey>>();
                let rate_limiter = rate_limiter.clone();
                
                async move {
                    rate_limiter.until_ready().await;  // Rate limit each chunk's RPC call
                    client.get_multiple_accounts(&pubkeys).await
                }
            })
            .collect();

        let futures_len = futures.len();
        let results = futures::future::join_all(futures).await;
        info!("Processing batch {}: {} accounts total ({} RPC calls)", 
            batch_num + 1, 
            chunks.len(),
            futures_len);
        
        for (chunk_num, (chunk, result)) in chunks.chunks(chunk_size).zip(results).enumerate() {
            if let Ok(accounts) = result {
                info!("  Processed chunk {}: {} accounts", chunk_num + 1, chunk.len());
                for ((owner, percentage), account) in chunk.iter().zip(accounts.iter()) {
                    if let Some(account) = account {
                        if program_ids.contains(account.owner.to_string().as_str()) {
                            info!("Excluded program-owned account {} holding {:.2}% of supply", 
                                owner.to_string(),
                                percentage * 100.0);
                            excluded_accounts.insert(*owner);
                        }
                    }
                }
            }
        }
    }

    info!("Found {} excluded accounts (program-owned + known LPs)", excluded_accounts.len());
    Ok(excluded_accounts)
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

pub async fn get_token_holders(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    mint_address: &str,
    price_in_usd: f64,
    _clickhouse_client: &Client,
) -> Result<TokenHolderStats, anyhow::Error> {
    rate_limiter.until_ready().await;
    let mint_pubkey = Pubkey::from_str(mint_address)?;
    let mint_account = client.get_account(&mint_pubkey).await?;
    let mint_data = spl_token::state::Mint::unpack(&mint_account.data)?;

    // Fetch the current price if not provided
    let price = if price_in_usd == 0.0 {
        get_token_price(mint_address).await?
    } else {
        price_in_usd
    };

    let supply = mint_data.supply as f64 / 10f64.powi(mint_data.decimals as i32);
    let market_cap = supply * price;

    // Specify the minimum balance here
    let min_balance = 1; // Example: set to 1 to exclude zero-balance accounts

    // Unpack the tuple returned by fetch_and_sort_holders
    let (holders, _) = fetch_and_sort_holders(client, &mint_pubkey, min_balance).await?;
    
    // Use only the holders vector for identify_program_owned_accounts
    let program_owned = identify_program_owned_accounts(client, rate_limiter, &holders, mint_data.supply, market_cap).await?;
    let final_holders: Vec<(String, u64)> = holders.into_iter()
        .filter(|(_, _, owner)| !program_owned.contains(owner))
        .map(|(pubkey, amount, _)| (pubkey, amount))
        .collect();

    // Calculate other metrics
    let result = calculate_holder_stats(&final_holders, &mint_data, price);
    Ok(result)
}

fn calculate_holder_stats(
    holders: &[(String, u64)],
    mint_data: &Mint,
    price_in_usd: f64,
) -> TokenHolderStats {
    let decimals = mint_data.decimals;
    let supply = mint_data.supply as f64 / 10f64.powi(decimals as i32);
    let market_cap = supply * price_in_usd;

    info!("=== Token Info ===");
    info!("  Price: ${}", price_in_usd);
    info!("  Supply: {:.2} tokens", supply);
    info!("  Market Cap: ${:.2}", market_cap);
    info!("  Decimals: {}", decimals);
    info!("  Total Holders for calculations: {}", holders.len());

    // Calculate minimum tokens needed for each USD threshold
    let usd_thresholds = vec![10.0, 100.0, 1000.0, 10000.0, 100000.0, 1000000.0];
    let min_tokens_for_threshold: Vec<u64> = usd_thresholds.iter()
        .map(|usd| ((usd / price_in_usd) * 10f64.powi(decimals as i32)) as u64)
        .collect();

    // Parallel threshold counting
    let threshold_counts: Vec<usize> = min_tokens_for_threshold.par_iter()
        .map(|threshold| {
            holders.par_iter()
                .filter(|(_, amount)| amount >= threshold)
                .count()
        })
        .collect();

    let total_holders = holders.len();
    let holders_above_10 = threshold_counts[0]; // Count of holders above $10

    let holder_thresholds: Vec<HolderThreshold> = usd_thresholds.iter()
        .zip(threshold_counts.iter())
        .map(|(usd, &count)| {
            let percentage_of_total = if total_holders > 0 {
                (count as f64 / total_holders as f64) * 100.0
            } else {
                0.0
            };

            let percentage_of_10 = if holders_above_10 > 0 {
                (count as f64 / holders_above_10 as f64) * 100.0
            } else {
                0.0
            };

            info!("${:.2} threshold:", usd);
            info!("  Required tokens: {:.2}", usd / price_in_usd);
            info!("  Holders meeting threshold: {} ({:.2}% of total [{} holders], {:.2}% of $10+ holders [{} holders])", 
                count,
                percentage_of_total,
                total_holders,
                percentage_of_10,
                holders_above_10
            );

            HolderThreshold {
                usd_threshold: *usd,
                count: count as i32,
                percentage: percentage_of_total,
                percentage_of_10,
            }
        })
        .collect();

    // Calculate concentration metrics using top 300 holders
    let mut sorted_holders = holders.to_vec();
    sorted_holders.sort_by(|a, b| b.1.cmp(&a.1));
    sorted_holders.truncate(300);  // Only keep top 300 for concentration metrics

    let concentration_points = vec![1, 10, 25, 50, 100, 250];
    
    let concentration_metrics: Vec<ConcentrationMetric> = concentration_points.par_iter()
        .map(|&n| {
            if n <= sorted_holders.len() {
                let sum: u64 = sorted_holders.par_iter()
                    .take(n)
                    .map(|(_, amount)| amount)
                    .sum();
                let percentage = (sum as f64 / mint_data.supply as f64) * 100.0;

                ConcentrationMetric {
                    top_n: n as i32,
                    percentage,
                }
            } else {
                ConcentrationMetric {
                    top_n: n as i32,
                    percentage: 0.0,
                }
            }
        })
        .collect();

    // Log results in order
    for metric in &concentration_metrics {
        info!("Top {} Holders: {:.2}%", metric.top_n, metric.percentage);
    }

    TokenHolderStats {
        price: price_in_usd,
        supply,
        market_cap,
        decimals,
        holders: total_holders,
        raw_holders: Some(holders.to_vec()),
        holder_thresholds,
        concentration_metrics,
        hhi: 0.0,
        distribution_score: 0.0,
    }
}

pub async fn calculate_distribution_metrics_async(
    holders: Vec<(String, u64)>,
    total_supply: u64,
) -> (f64, f64) {
    // Spawn the heavy computation in a blocking task
    tokio::task::spawn_blocking(move || {
        let total_supply_f64 = total_supply as f64;
        
        // Calculate HHI
        let hhi: f64 = holders.iter()
            .map(|(_, amount)| {
                let market_share = (*amount as f64 / total_supply_f64) * 100.0;
                market_share * market_share
            })
            .sum();

        // Calculate distribution score
        let distribution_score = calculate_distribution_score(&holders, total_supply);

        (hhi, distribution_score)
    })
    .await
    .unwrap_or((0.0, 0.0))
}

fn calculate_distribution_score(holders: &[(String, u64)], total_supply: u64) -> f64 {
    if holders.is_empty() {
        return 0.0;
    }

    let mut sorted_holders = holders.to_vec();
    sorted_holders.sort_by(|a, b| a.1.cmp(&b.1)); // Sort in ascending order

    let n = sorted_holders.len() as f64;
    let total_supply_f64 = total_supply as f64;
    
    let mut numerator = 0.0;
    for (_i, (_, amount)) in sorted_holders.iter().enumerate() {
        let amount_f64 = *amount as f64;
        for (_, other_amount) in sorted_holders.iter() {
            let other_amount_f64 = *other_amount as f64;
            numerator += (amount_f64 - other_amount_f64).abs();
        }
    }

    // Calculate Gini coefficient
    let gini = numerator / (2.0 * n * n * total_supply_f64 / n);

    // Convert to distribution score (0-100)
    let distribution_score = (1.0 - gini) * 100.0;

    distribution_score.clamp(0.0, 100.0)
}

pub async fn update_token_metrics(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    mint_address: &str,
    clickhouse_client: &Client,
) -> Result<(), anyhow::Error> {
    let stats = get_token_holders(client, rate_limiter, mint_address, 0.0, clickhouse_client).await?;
    
    // Get raw supply from mint data
    let mint_pubkey = Pubkey::from_str(mint_address)?;
    let mint_account = client.get_account(&mint_pubkey).await?;
    let mint_data = spl_token::state::Mint::unpack(&mint_account.data)?;
    
    let (hhi, distribution_score) = calculate_distribution_metrics_async(stats.raw_holders.unwrap(), mint_data.supply).await;
    
    insert_distribution_metrics(clickhouse_client, mint_address, hhi, distribution_score).await?;
    
    Ok(())
}
