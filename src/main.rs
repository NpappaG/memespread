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
    min_balance: u64,
) -> Result<Vec<(String, u64)>, Box<dyn std::error::Error>> {
    let mint_pubkey = Pubkey::from_str(mint_address)?;

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
            if token_account.amount > min_balance {
                token_holders.push((pubkey.to_string(), token_account.amount));
            }
        }
    }

    Ok(token_holders)
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize the logger
    tracing_subscriber::fmt::init();
    
    dotenv().ok();
    let api_key = env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY must be set");
    let rpc_url = format!("https://rpc.helius.xyz/?api-key={}", api_key);
    let rpc_client = RpcClient::new(rpc_url);
    let mint_address = "3an8rhdepsLCya22af7qDBKPbdomw8K4iCHXaA2Gpump";
    let min_balance: u64 = 14450867052;

    let token_holders = get_token_holders(&rpc_client, &mint_address, min_balance)
        .await
        .expect("Failed to fetch token holders");

    for (address, balance) in &token_holders {
        tracing::info!("Address: {}, Balance: {}", address, balance);
    }
    tracing::info!("Token Mint: {:?}", mint_address);
    tracing::info!("Token holders with balance > {}:", min_balance);
    tracing::info!("Number of hodlers: {:?}", token_holders.len());

    Ok(())
}