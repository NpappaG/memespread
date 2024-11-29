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

async fn get_token_holders(
    client: &RpcClient,
    mint_address: &str,
    price_in_usd: f64,
) -> Result<Vec<(String, u64)>, Box<dyn std::error::Error>> {
    let mint_pubkey = Pubkey::from_str(mint_address)?;
    let mint_account = client.get_account(&mint_pubkey).await?;
    let mint_data = spl_token::state::Mint::unpack(&mint_account.data)?;
    let decimals = mint_data.decimals;

    let thresholds = vec![10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0];
    let mut threshold_counts = vec![0; thresholds.len()];
    
    let min_tokens_for_threshold: Vec<f64> = thresholds.iter()
        .map(|usd| usd / price_in_usd)
        .collect();

    tracing::info!("Price in USD: ${}", price_in_usd);
    for (_i, (usd, tokens)) in thresholds.iter().zip(&min_tokens_for_threshold).enumerate() {
        tracing::info!("Tokens needed for ${}: {}", usd, tokens);
    }

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
    for (pubkey, account) in accounts {
        if let Ok(token_account) = TokenAccount::unpack(&account.data) {
            let token_amount_in_tokens = (token_account.amount as f64) / (10f64.powi(decimals as i32));
            
            // Update counts for each threshold
            for (i, threshold) in min_tokens_for_threshold.iter().enumerate() {
                if token_amount_in_tokens >= *threshold {
                    threshold_counts[i] += 1;
                }
            }

            if token_amount_in_tokens >= min_tokens_for_threshold.last().unwrap().clone() {
                token_holders.push((pubkey.to_string(), token_account.amount));
            }
        }
    }

    // After counting all holders, calculate total holders first
    let total_holders = threshold_counts[0]; // Using $10+ as total holder count

    // Then modify the logging loop
    for (_i, (usd, count)) in thresholds.iter().zip(&threshold_counts).enumerate() {
        let percentage = (*count as f64 / total_holders as f64) * 100.0;
        tracing::info!("Holders with ${:.2}+: {} ({:.2}% of holders)", usd, count, percentage);
    }

    // Sort by amount in descending order
    token_holders.sort_by(|a, b| b.1.cmp(&a.1));

    //tracing::info!("Found {} holders with $10k+ worth", token_holders.len());
    Ok(token_holders)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    dotenv().ok();
    let api_key = env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY must be set");
    let rpc_url = format!("https://rpc.helius.xyz/?api-key={}", api_key);
    let rpc_client = RpcClient::new(rpc_url);
    let mint_address = "7qBKePC5SqZKDRNsbNhqD6Y6S8JW2CM3KoRv3ztDpump";
    
    let price_in_usd = 0.002416;

    let holders = get_token_holders(&rpc_client, &mint_address, price_in_usd)
        .await
        .expect("Failed to fetch token holders");

    //for (address, amount) in &holders {
    //    tracing::info!("Holder address: {}, amount: {}", address, amount);
    //}
    tracing::info!("Total $10k+ holders: {}", holders.len());

    Ok(())
}