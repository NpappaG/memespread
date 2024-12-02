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

async fn fetch_and_sort_holders(
    client: &Arc<RpcClient>,
    mint_pubkey: &Pubkey,
) -> Result<Vec<(String, u64, Pubkey)>, anyhow::Error> {
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
                .map(|token_account| (pubkey.to_string(), token_account.amount, token_account.owner))
        })
        .collect();
    
    holders.sort_by(|a, b| b.1.cmp(&a.1));
    Ok(holders)
}

async fn identify_program_owned_accounts(
    client: &Arc<RpcClient>,
    holders: &[(String, u64, Pubkey)],
    mint_supply: u64,
) -> Result<HashSet<Pubkey>, anyhow::Error> {
    let program_ids: HashSet<&str> = vec![
        "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium concentrated
        "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo", // Meteora DLMM
    ].into_iter().collect();

    let excluded_owners: HashSet<&str> = vec![
        "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", //Raydium LP
    ].into_iter().collect();

    let threshold = 0.001;
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

    let holders = fetch_and_sort_holders(client, &mint_pubkey).await?;
    let program_owned = identify_program_owned_accounts(client, &holders, mint_data.supply).await?;
    let final_holders: Vec<(String, u64)> = holders.into_iter()
        .filter(|(_, _, owner)| {
            !program_owned.contains(owner)
        })
        .map(|(pubkey, amount, _)| (pubkey, amount))
        .collect();

    Ok(calculate_holder_stats(&final_holders, &mint_data, price))
}

fn calculate_holder_stats(
    holders: &[(String, u64)],
    mint_data: &Mint,
    price_in_usd: f64,
) -> TokenHolderStats {
    let decimals = mint_data.decimals;
    let supply = mint_data.supply as f64 / 10f64.powi(decimals as i32);
    let market_cap = supply * price_in_usd;

    // Log token info
    tracing::info!("=== Token Info ===");
    tracing::info!("  Price: ${}", price_in_usd);
    tracing::info!("  Supply: {:.2} tokens", supply);
    tracing::info!("  Market Cap: ${:.2}", market_cap);
    tracing::info!("  Decimals: {}", decimals);

    // Calculate minimum tokens needed for each USD threshold
    let usd_thresholds = vec![10.0, 100.0, 1000.0, 10000.0, 100000.0, 1000000.0];
    let min_tokens_for_threshold: Vec<u64> = usd_thresholds.iter()
        .map(|usd| ((usd / price_in_usd) * 10f64.powi(decimals as i32)) as u64)
        .collect();

    // Count holders meeting each threshold
    let mut threshold_counts = vec![0; usd_thresholds.len()];
    for (_, amount) in holders {
        for (i, threshold) in min_tokens_for_threshold.iter().enumerate() {
            if amount >= threshold {
                threshold_counts[i] += 1;
            }
        }
    }

    let total_holders = holders.len();
    let holder_thresholds: Vec<HolderThreshold> = usd_thresholds.iter()
        .zip(threshold_counts.iter())
        .map(|(usd, &count)| {
            let percentage = if total_holders > 0 {
                (count as f64 / total_holders as f64) * 100.0
            } else {
                0.0
            };
            
            tracing::info!("${:.2} threshold:", usd);
            tracing::info!("  Required tokens: {:.2}", usd / price_in_usd);
            tracing::info!("  Holders meeting threshold: {} ({:.2}%)", count, percentage);

            HolderThreshold {
                usd_threshold: *usd,
                count: count as i32,
                percentage,
            }
        })
        .collect();

    // Calculate concentration metrics using top 300 holders
    let mut sorted_holders = holders.to_vec();
    sorted_holders.sort_by(|a, b| b.1.cmp(&a.1));

    let concentration_points = vec![1, 10, 25, 50, 100, 250];
    let concentration_metrics: Vec<ConcentrationMetric> = concentration_points.iter()
        .map(|&n| {
            if n <= sorted_holders.len() {
                let sum: u64 = sorted_holders.iter().take(n).map(|(_, amount)| amount).sum();
                let percentage = (sum as f64 / mint_data.supply as f64) * 100.0;
                tracing::info!("Top {} Holders: {:.2}%", n, percentage);

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

    TokenHolderStats {
        price: price_in_usd,
        supply,
        market_cap,
        decimals,
        holder_thresholds,
        concentration_metrics,
    }
}
