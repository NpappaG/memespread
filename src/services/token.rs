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
use tracing::info;
use chrono::Utc;
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
    tracing::info!("Found {} total token accounts", accounts.len());

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
    tracing::info!("Found {} accounts with balance > {}", holder_count, min_balance);

    let unique_owners: HashSet<_> = holders.iter().map(|(_, _, owner)| owner).collect();
    tracing::info!("Found {} unique owners with balance > {}", unique_owners.len(), min_balance);
    
    holders.sort_by(|a, b| b.1.cmp(&a.1));
    Ok((holders, holder_count))
}

async fn identify_program_owned_accounts(
    client: &Arc<RpcClient>,
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

    tracing::info!("Using threshold of {}% based on ${:.2}M market cap", 
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

    tracing::info!("Found {} holders above {}%", large_holders.len(), threshold * 100.0);

    let mut excluded_accounts = HashSet::new();
    let chunk_size = 25;

    // First, add known excluded owners
    for (owner, percentage) in &large_holders {
        let owner_str = owner.to_string();
        if excluded_owners.contains(owner_str.as_str()) {
            tracing::info!("Excluded known LP {} holding {:.2}% of supply", 
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
                async move {
                    client.get_multiple_accounts(&pubkeys).await
                }
            })
            .collect();

        let futures_len = futures.len();
        let results = futures::future::join_all(futures).await;
        tracing::info!("Processing batch {}: {} accounts total ({} RPC calls)", 
            batch_num + 1, 
            chunks.len(),
            futures_len);
        
        for (chunk_num, (chunk, result)) in chunks.chunks(chunk_size).zip(results).enumerate() {
            if let Ok(accounts) = result {
                tracing::info!("  Processed chunk {}: {} accounts", chunk_num + 1, chunk.len());
                for ((owner, percentage), account) in chunk.iter().zip(accounts.iter()) {
                    if let Some(account) = account {
                        if program_ids.contains(account.owner.to_string().as_str()) {
                            tracing::info!("Excluded program-owned account {} holding {:.2}% of supply", 
                                owner.to_string(),
                                percentage * 100.0);
                            excluded_accounts.insert(*owner);
                        }
                    }
                }
            }
        }
    }

    tracing::info!("Found {} excluded accounts (program-owned + known LPs)", excluded_accounts.len());
    Ok(excluded_accounts)
}

async fn get_token_price(mint_address: &str) -> Result<f64, anyhow::Error> {
    let url = format!("https://api.jup.ag/price/v2?ids={}", mint_address);
    let response = reqwest::get(url).await?;
    let json: Value = response.json().await?;
    
    tracing::info!("Jupiter API response: {:?}", json);
    
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
) -> Result<TokenHolderStats, anyhow::Error> {
    let operation_start = std::time::Instant::now();

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
    let (holders, holder_count) = fetch_and_sort_holders(client, &mint_pubkey, min_balance).await?;
    
    // Use only the holders vector for identify_program_owned_accounts
    let program_owned = identify_program_owned_accounts(client, &holders, mint_data.supply, market_cap).await?;
    let final_holders: Vec<(String, u64)> = holders.into_iter()
        .filter(|(_, _, owner)| !program_owned.contains(owner))
        .map(|(pubkey, amount, _)| (pubkey, amount))
        .collect();

    // Log the final holder count
    tracing::info!("Final holder count (accounts with balance > 1): {}", final_holders.len());

    // Calculate other metrics while distribution metrics are being computed
    let result = calculate_holder_stats(&final_holders, &mint_data, price);
    
    // Instead of awaiting, spawn the task and return immediately
    let holders_for_distro = final_holders.clone();
    let supply_for_distro = mint_data.supply;
    
    // Spawn the distribution calculation as a separate task
    tokio::spawn(async move {
        let distro_start = std::time::Instant::now();
        let (_hhi, _distribution_score) = calculate_distribution_metrics_async(holders_for_distro, supply_for_distro).await;
        let distro_duration = distro_start.elapsed();
        tracing::info!("Distribution metrics calculation took: {:?}", distro_duration);
        
        // Here you would send the results to a cache, database, or websocket
        // Example: cache.set(format!("distro_metrics:{}", mint_address), (hhi, distribution_score)).await;
    });

    let token_holder_stats = TokenHolderStats {
        price,
        supply: supply as f64,
        market_cap,
        decimals: mint_data.decimals,
        holders: holder_count,
        holder_thresholds: result.holder_thresholds,
        concentration_metrics: result.concentration_metrics,
        hhi: 0.0, // Will be updated later
        distribution_score: 0.0, // Will be updated later
    };

    let total_duration = operation_start.elapsed();
    tracing::info!("Initial stats calculation took: {:?}", total_duration);
    
    let current_timestamp = Utc::now().naive_utc()
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    
    // Create ClickHouse client
    let clickhouse_client = Client::default()
        .with_url("http://localhost:8123")
        .with_database("default");

    info!("Attempting to save stats to ClickHouse for mint address: {}", mint_address);
    clickhouse_client
        .query(
            "INSERT INTO monitored_tokens (mint_address, last_stats_update, last_metrics_update) VALUES (?, ?, ?)"
        )
        .bind(mint_address)
        .bind(&current_timestamp)
        .bind(&current_timestamp)
        .execute()
        .await?;
    info!("Successfully saved stats to ClickHouse for mint address: {}", mint_address);
    
    Ok(token_holder_stats)
}

fn calculate_holder_stats(
    holders: &[(String, u64)],
    mint_data: &Mint,
    price_in_usd: f64,
) -> TokenHolderStats {
    let decimals = mint_data.decimals;
    let supply = mint_data.supply as f64 / 10f64.powi(decimals as i32);
    let market_cap = supply * price_in_usd;

    tracing::info!("=== Token Info ===");
    tracing::info!("  Price: ${}", price_in_usd);
    tracing::info!("  Supply: {:.2} tokens", supply);
    tracing::info!("  Market Cap: ${:.2}", market_cap);
    tracing::info!("  Decimals: {}", decimals);
    tracing::info!("  Total Holders for calculations: {}", holders.len());

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

            tracing::info!("${:.2} threshold:", usd);
            tracing::info!("  Required tokens: {:.2}", usd / price_in_usd);
            tracing::info!("  Holders meeting threshold: {} ({:.2}% of total [{} holders], {:.2}% of $10+ holders [{} holders])", 
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
        tracing::info!("Top {} Holders: {:.2}%", metric.top_n, metric.percentage);
    }

    TokenHolderStats {
        price: price_in_usd,
        supply,
        market_cap,
        decimals,
        holders: total_holders,
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