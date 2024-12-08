use std::sync::Arc;
use tokio::time::Duration;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
use clickhouse::Client;
use std::str::FromStr;

pub const PROGRAM_IDS: &[&str] = &[
    "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium concentrated
    "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo", // Meteora DLMM
    "whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc", //Orca

];

pub const EXCLUDED_OWNERS: &[&str] = &[
    "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", //Raydium LP
    "u6PJ8DtQuPFnfmwHbGFULQ4u4EgjDiyYKjVEsynXq2w", // Gateio
    "A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR", //bitget
    "HVh6wHNBAsG3pq1Bj5oCzRjoWKVogEDHwUHkRz3ekFgt", //Kucoin
    "ASTyfSima4LLAdDgoFGkgqoKowG1LZFDr9fAQrg7iaJZ", //MEXC
    "5PAhQiYdLBd6SVdjzBQDxUAEFyDdF5ExNPQfcscnPRj5", //MEXC #2
    "3ADzk5YDP9sgorvPSs9YPxigJiSqhgddpwHwwPwmEFib", //Binance Deposit
    "5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9", //Binance #2
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM", //Binance #3
    "AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2", //Bybit
    "FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5", //Kraken
    "9cNE6KBg2Xmf34FPMMvzDF8yUHMrgLRzBV3vD7b1JnUS", //Kraken Deposit
    "GugU1tP7doLeTw9hQP51xRJyS8Da1fWxuiy2rVrnMD2m", //Wormhole Custody
    "9un5wqE3q4oCjyrDkwsdD48KteCJitQX5978Vh7KKxHo", //OKX2
    "6FEVkH17P9y8Q9aCkDdPcMDjvj7SVxrTETaYEm8f51Jy", //Crypto.com #1

]; 

pub async fn update_excluded_accounts(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    clickhouse_client: &Client,
) -> Result<(), anyhow::Error> {
    // First, insert known exclusions
    for &address in EXCLUDED_OWNERS {
        clickhouse_client
            .query("
                INSERT INTO excluded_accounts (address, category, description)
                VALUES (?, 'exchange', 'Known CEX/LP')
            ")
            .bind(address)
            .execute()
            .await?;
    }

    // Get top 300 holders across all monitored tokens
    let large_holders: Vec<(String, f64)> = clickhouse_client
        .query("
            SELECT DISTINCT holder_address, sum(total_amount) as total
            FROM token_holder_balances_mv
            GROUP BY holder_address
            ORDER BY total DESC
            LIMIT 300
        ")
        .fetch_all()
        .await?;

    // Check if they're program accounts in batches
    for chunk in large_holders.chunks(25) {
        rate_limiter.until_ready().await;
        
        let addresses: Vec<Pubkey> = chunk.iter()
            .filter_map(|(addr, _)| Pubkey::from_str(addr).ok())
            .collect();
            
        if let Ok(accounts) = client.get_multiple_accounts(&addresses).await {
            for (account, (address, _)) in accounts.iter().zip(chunk.iter()) {
                if let Some(acc) = account {
                    if PROGRAM_IDS.contains(&acc.owner.to_string().as_str()) {
                        clickhouse_client
                            .query("
                                INSERT INTO excluded_accounts (address, category, description)
                                VALUES (?, 'program', ?)
                            ")
                            .bind(address)
                            .bind(format!("Owned by {}", acc.owner))
                            .execute()
                            .await?;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

pub async fn check_new_token_exclusions(
    client: &Arc<RpcClient>,
    rate_limiter: &Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    clickhouse_client: &Client,
    mint_address: &str,
) -> Result<(), anyhow::Error> {
    // First, insert known exclusions
    for &address in EXCLUDED_OWNERS {
        clickhouse_client
            .query("
                INSERT INTO excluded_accounts (address, category, description)
                VALUES (?, 'exchange', 'Known CEX/LP')
            ")
            .bind(address)
            .execute()
            .await?;
    }

    // Get top holders for just this token
    let large_holders: Vec<(String, f64)> = clickhouse_client
        .query("
            SELECT DISTINCT holder_address, total_amount
            FROM token_holder_balances_mv
            WHERE mint_address = ?
            ORDER BY total_amount DESC
            LIMIT 300
        ")
        .bind(mint_address)
        .fetch_all()
        .await?;

    // Check if they're program accounts in batches
    for chunk in large_holders.chunks(25) {
        rate_limiter.until_ready().await;
        
        let addresses: Vec<Pubkey> = chunk.iter()
            .filter_map(|(addr, _)| Pubkey::from_str(addr).ok())
            .collect();
            
        if let Ok(accounts) = client.get_multiple_accounts(&addresses).await {
            for (account, (address, _)) in accounts.iter().zip(chunk.iter()) {
                if let Some(acc) = account {
                    if PROGRAM_IDS.contains(&acc.owner.to_string().as_str()) {
                        clickhouse_client
                            .query("
                                INSERT INTO excluded_accounts (address, category, description)
                                VALUES (?, 'program', ?)
                            ")
                            .bind(address)
                            .bind(format!("Owned by {}", acc.owner))
                            .execute()
                            .await?;
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

// Keep the periodic update for catching any missed ones
pub async fn schedule_exclusion_updates(
    client: Arc<RpcClient>,
    rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    clickhouse_client: Client,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(24 * 60 * 60));
    loop {
        interval.tick().await;
        if let Err(e) = update_excluded_accounts(&client, &rate_limiter, &clickhouse_client).await {
            tracing::error!("Failed to update excluded accounts: {}", e);
        }
    }
} 