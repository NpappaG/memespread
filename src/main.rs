use std::str::FromStr;
use dotenv::dotenv;
use std::env;

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
use spl_token::state::Account as TokenAccount;
use reqwest;
use serde_json::Value;

async fn get_token_price(mint_address: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let url = format!("https://api.jup.ag/price/v2?ids={}", mint_address);
    let response = reqwest::get(url).await?;
    let json: Value = response.json().await?;
    
    // Log the full response
    tracing::info!("Jupiter API response: {:?}", json);
    
    // Extract price from Jupiter response
    let price = json["data"][mint_address]["price"]
        .as_str()
        .ok_or("Failed to parse price")?
        .parse::<f64>()?;
    
    Ok(price)
}

async fn get_token_holders(
    client: &RpcClient,
    mint_address: &str,
    price_in_usd: f64,
) -> Result<Vec<(String, u64)>, Box<dyn std::error::Error>> {
    let mint_pubkey = Pubkey::from_str(mint_address)?;
    let mint_account = client.get_account(&mint_pubkey).await?;
    let mint_data = spl_token::state::Mint::unpack(&mint_account.data)?;
    let decimals = mint_data.decimals;
    
    // Calculate supply and market cap
    let supply = mint_data.supply as f64 / 10f64.powi(decimals as i32);
    let market_cap = supply * price_in_usd;

    let thresholds = vec![10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0];
    let mut threshold_counts = vec![0; thresholds.len()];
    
    let min_tokens_for_threshold: Vec<f64> = thresholds.iter()
        .map(|usd| usd / price_in_usd)
        .collect();

    let accounts = client.get_program_accounts_with_config(
        &spl_token::ID,
        solana_client::rpc_config::RpcProgramAccountsConfig {
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
        },
    ).await?;

    let mut token_holders = Vec::new();
    let excluded_owners = vec![
        // Raydium LP
        "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", // Raydium LP
        "7aV4BodfLwhrwyqsLXCoMmxXzCzdEPpz3QMv46XjQsFb", // Meteora
    ];
    
    for (pubkey, account) in accounts {
        if let Ok(token_account) = TokenAccount::unpack(&account.data) {
            // Skip if owned by any excluded address
            if excluded_owners.contains(&token_account.owner.to_string().as_str()) {
                tracing::info!("Excluded {} holding {} tokens", 
                    token_account.owner.to_string(),
                    token_account.amount as f64 / 10f64.powi(decimals as i32));
                continue;
            }
            
            token_holders.push((pubkey.to_string(), token_account.amount));
            
            let token_amount_in_tokens = (token_account.amount as f64) / (10f64.powi(decimals as i32));
            for (i, threshold) in min_tokens_for_threshold.iter().enumerate() {
                if token_amount_in_tokens >= *threshold {
                    threshold_counts[i] += 1;
                }
            }
        }
    }

    token_holders.sort_by(|a, b| b.1.cmp(&a.1));
    let total_supply = mint_data.supply;
    
    // First show token info
    tracing::info!("=== Token Info ===");
    tracing::info!("  Price: ${}", price_in_usd);
    tracing::info!("  Supply: {:.2} tokens", supply);
    tracing::info!("  Market Cap: ${:.2}", market_cap);
    tracing::info!("  Decimals: {}", decimals);

    // Then show holder stratification
    tracing::info!("=== Holder Stratification ===");
    for (_i, (usd, count)) in thresholds.iter().zip(&threshold_counts).enumerate() {
        let percentage = (*count as f64 / threshold_counts[0] as f64) * 100.0;
        tracing::info!("Holders with ${:.2}+: {} ({:.2}% of holders)", usd, count, percentage);
    }

    // Finally show concentration metrics
    tracing::info!("=== Holder Concentration ===");
    let concentration_points = vec![1, 10, 25, 50, 100, 250];
    for &n in &concentration_points {
        if n <= token_holders.len() {
            let sum: u64 = token_holders.iter().take(n).map(|(_, amount)| amount).sum();
            let percentage = (sum as f64 / total_supply as f64) * 100.0;
            tracing::info!("Top {} Holders: {:.2}%", n, percentage);
        }
    }

    Ok(token_holders)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    dotenv().ok();
    let api_key = env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY must be set");
    let rpc_url = format!("https://rpc.helius.xyz/?api-key={}", api_key);
    let rpc_client = RpcClient::new(rpc_url);
    let mint_address = "F9GqoJRPzQnGzvP7cQzLHB7C22DToHQYWfsPvhKwqrpC";
    
    let price_in_usd = get_token_price(mint_address).await.expect("Failed to fetch token price");

    get_token_holders(&rpc_client, &mint_address, price_in_usd)
        .await
        .expect("Failed to fetch token holders");

    Ok(())
}