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

fn filter_final_holders(
    holders: Vec<(String, u64, Pubkey)>,
    excluded_accounts: HashSet<Pubkey>,
    mint_supply: u64,
) -> Vec<(String, u64)> {
    holders.into_iter()
        .filter(|(_, amount, owner)| {
            let percentage = (*amount as f64) / (mint_supply as f64);
            if percentage < 0.001 { return true; }
            !excluded_accounts.contains(owner)
        })
        .map(|(pubkey, amount, _)| (pubkey, amount))
        .take(300)
        .collect()
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

    let holders = fetch_and_sort_holders(client, &mint_pubkey).await?;
    let program_owned = identify_program_owned_accounts(client, &holders, mint_data.supply).await?;
    let final_holders = filter_final_holders(holders, program_owned, mint_data.supply);

    Ok(calculate_holder_stats(&final_holders, &mint_data, price_in_usd))
}

fn calculate_holder_stats(
    holders: &[(String, u64)],
    mint_data: &Mint,
    price_in_usd: f64,
) -> TokenHolderStats {
    let supply = mint_data.supply as f64 / 10f64.powi(mint_data.decimals as i32);
    let market_cap = supply * price_in_usd;

    // Calculate holder thresholds ($100, $1000, $10000, etc.)
    let thresholds = vec![100.0, 1000.0, 10000.0, 100000.0, 1000000.0];
    let holder_thresholds: Vec<HolderThreshold> = thresholds.iter().map(|threshold| {
        let token_amount = threshold / price_in_usd;
        let raw_amount = (token_amount * 10f64.powi(mint_data.decimals as i32)) as u64;
        
        let count = holders.iter()
            .filter(|(_, amount)| *amount >= raw_amount)
            .count();

        HolderThreshold {
            usd_threshold: *threshold,
            count: count as i32,
            percentage: (count as f64 / holders.len() as f64) * 100.0,
        }
    }).collect();

    // Calculate concentration metrics (top 10, 50, 100, etc.)
    let top_ns = vec![10, 50, 100, 200];
    let concentration_metrics: Vec<ConcentrationMetric> = top_ns.iter().map(|&n| {
        let sum: u64 = holders.iter()
            .take(n as usize)
            .map(|(_, amount)| *amount)
            .sum();
        
        ConcentrationMetric {
            top_n: n,
            percentage: (sum as f64 / mint_data.supply as f64) * 100.0,
        }
    }).collect();

    TokenHolderStats {
        price: price_in_usd,
        supply,
        market_cap,
        decimals: mint_data.decimals,
        holder_thresholds,
        concentration_metrics,
    }
}