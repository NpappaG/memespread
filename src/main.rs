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
use rayon::prelude::*;
use std::collections::HashSet;
use axum::{
    routing::get,
    Router,
    extract::Query,
    Json,
    extract::State,
    http::{Method, StatusCode, HeaderValue},
    debug_handler,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use std::net::SocketAddr;
use serde_json::json;
use std::sync::Arc;
use tokio::net::TcpListener;

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
    client: &Arc<RpcClient>,
    mint_address: &str,
    price_in_usd: f64,
) -> Result<Vec<(String, u64)>, anyhow::Error> {
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

    // Convert accounts to parallel iterator for initial processing
    let mut token_holders: Vec<(String, u64, Pubkey)> = accounts.into_par_iter()
        .filter_map(|(pubkey, account)| {
            TokenAccount::unpack(&account.data).ok()
                .map(|token_account| (pubkey.to_string(), token_account.amount, token_account.owner))
        })
        .collect();
    
    token_holders.sort_by(|a, b| b.1.cmp(&a.1));  // Sort by amount (descending)

    let mut final_holders = Vec::new();
    let program_ids: HashSet<&str> = vec![
        "CAMMCzo5YL8w4VFF8KVHrK22GGUsp5VTaW7grrKgrWqK", // Raydium concentrated
        "LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo", // Meteora DLMM
    ].into_iter().collect();

    let excluded_owners: HashSet<&str> = vec![
        "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1", //Raydium LP
    ].into_iter().collect();

    let threshold = 0.001; // This is 0.1%

    // Get all holders above threshold
    let large_holders: Vec<Pubkey> = token_holders.iter()
        .filter(|(_, amount, _)| {
            let percentage = (*amount as f64) / (mint_data.supply as f64);
            percentage >= threshold
        })
        .map(|(_, _, owner)| *owner)
        .collect();

    tracing::info!("Found {} holders above {}%", large_holders.len(), threshold * 100.0);

    // Process in smaller chunks
    let chunk_size = 25; // Reduced from 100 to 25
    let mut program_owned_accounts = HashSet::new();

    let mut rpc_calls = 0;
    
    // Process chunks concurrently in groups of 4 (100 total)
    for (batch_num, chunks) in large_holders.chunks(chunk_size * 4).enumerate() {
        let mut futures = vec![];
        
        // Create futures for each chunk
        for chunk in chunks.chunks(chunk_size) {
            futures.push(client.get_multiple_accounts(chunk));
            rpc_calls += 1;
        }

        let futures_len = futures.len();  // Capture length before move
        
        // Execute chunks concurrently
        let results = futures::future::join_all(futures).await;
        tracing::info!("Processing batch {}: {} accounts total ({} RPC calls)", 
            batch_num + 1, 
            chunks.len(),
            futures_len);  // Use captured length
        
        // Process results
        for (chunk_num, (chunk, result)) in chunks.chunks(chunk_size).zip(results).enumerate() {
            if let Ok(accounts_batch) = result {
                tracing::info!("  Processed chunk {}: {} accounts", chunk_num + 1, chunk.len());
                chunk.iter()
                    .zip(accounts_batch.iter())
                    .filter_map(|(owner, account_opt)| {
                        account_opt.as_ref().and_then(|account| {
                            if program_ids.contains(account.owner.to_string().as_str()) {
                                Some(*owner)
                            } else {
                                None
                            }
                        })
                    })
                    .for_each(|owner| { program_owned_accounts.insert(owner); });
            }
        }
    }
    
    tracing::info!("Total RPC calls made: {}", rpc_calls);

    tracing::info!("Found {} program-owned accounts", program_owned_accounts.len());

    // Process all holders with cached program checks
    for (pubkey, amount, owner) in token_holders {
        let percentage_of_supply = (amount as f64) / (mint_data.supply as f64);
        
        if percentage_of_supply < 0.001 {
            final_holders.push((pubkey, amount));
            continue;
        }

        let owner_str = owner.to_string();
        if excluded_owners.contains(owner_str.as_str()) {
            tracing::info!("Excluded known LP {} holding {:.2}% of supply", 
                owner_str,
                percentage_of_supply * 100.0);
            continue;
        }

        if program_owned_accounts.contains(&owner) {
            tracing::info!("Excluded program-owned account {} holding {:.2}% of supply", 
                owner_str,
                percentage_of_supply * 100.0);
            continue;
        }

        final_holders.push((pubkey, amount));
    }

    // Update threshold counts
    for (_, amount) in &final_holders {
        let token_amount_in_tokens = (*amount as f64) / (10f64.powi(decimals as i32));
        for (i, threshold) in min_tokens_for_threshold.iter().enumerate() {
            if token_amount_in_tokens >= *threshold {
                threshold_counts[i] += 1;
            }
        }
    }

    final_holders.sort_by(|a, b| b.1.cmp(&a.1));
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
        if n <= final_holders.len() {
            let sum: u64 = final_holders.iter().take(n).map(|(_, amount)| amount).sum();
            let percentage = (sum as f64 / total_supply as f64) * 100.0;
            tracing::info!("Top {} Holders: {:.2}%", n, percentage);
        }
    }

    Ok(final_holders)
}

#[derive(Deserialize)]
struct TokenQuery {
    mint_address: String,
}

#[derive(Serialize)]
struct TokenHolderStats {
    price: f64,
    supply: f64,
    market_cap: f64,
    decimals: u8,
    holder_thresholds: Vec<HolderThreshold>,
    concentration_metrics: Vec<ConcentrationMetric>,
}

#[derive(Serialize)]
struct HolderThreshold {
    usd_threshold: f64,
    count: usize,
    percentage: f64,
}

#[derive(Serialize)]
struct ConcentrationMetric {
    top_n: usize,
    percentage: f64,
}

#[debug_handler]
async fn token_stats(
    Query(params): Query<TokenQuery>,
    State(rpc_client): State<Arc<RpcClient>>,
) -> Result<Json<TokenHolderStats>, (StatusCode, Json<serde_json::Value>)> {
    let price_in_usd = match get_token_price(&params.mint_address).await {
        Ok(price) => price,
        Err(e) => {
            return Err((StatusCode::BAD_REQUEST, Json(json!({
                "error": format!("Failed to fetch price: {}", e)
            }))));
        }
    };

    let holders = get_token_holders(&rpc_client, &params.mint_address, price_in_usd)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": format!("Failed to fetch holders: {}", e)
            })))
        })?;

    let mint_pubkey = Pubkey::from_str(&params.mint_address)
        .map_err(|e| {
            (StatusCode::BAD_REQUEST, Json(json!({
                "error": format!("Invalid mint address: {}", e)
            })))
        })?;

    let mint_account = rpc_client.get_account(&mint_pubkey)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": format!("Failed to fetch mint account: {}", e)
            })))
        })?;

    let mint_data = spl_token::state::Mint::unpack(&mint_account.data)
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": format!("Failed to unpack mint data: {}", e)
            })))
        })?;

    let decimals = mint_data.decimals;
    let supply = mint_data.supply as f64 / 10f64.powi(decimals as i32);
    let market_cap = supply * price_in_usd;

    // Calculate holder thresholds
    let thresholds = vec![10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0];
    let min_tokens_for_threshold: Vec<f64> = thresholds.iter()
        .map(|usd| usd / price_in_usd)
        .collect();

    let mut threshold_counts = vec![0; thresholds.len()];
    for (_, amount) in &holders {
        let token_amount = (*amount as f64) / (10f64.powi(decimals as i32));
        for (i, threshold) in min_tokens_for_threshold.iter().enumerate() {
            if token_amount >= *threshold {
                threshold_counts[i] += 1;
            }
        }
    }

    let holder_thresholds: Vec<HolderThreshold> = thresholds.iter()
        .zip(&threshold_counts)
        .map(|(usd, count)| HolderThreshold {
            usd_threshold: *usd,
            count: *count,
            percentage: if threshold_counts[0] > 0 {
                (*count as f64 / threshold_counts[0] as f64) * 100.0
            } else {
                0.0
            },
        })
        .collect();

    // Calculate concentration metrics
    let concentration_points = vec![1, 10, 25, 50, 100, 250];
    let concentration_metrics: Vec<ConcentrationMetric> = concentration_points.iter()
        .filter_map(|&n| {
            if n <= holders.len() {
                let sum: u64 = holders.iter().take(n).map(|(_, amount)| amount).sum();
                let percentage = (sum as f64 / mint_data.supply as f64) * 100.0;
                Some(ConcentrationMetric {
                    top_n: n,
                    percentage,
                })
            } else {
                None
            }
        })
        .collect();

    let stats = TokenHolderStats {
        price: price_in_usd,
        supply,
        market_cap,
        decimals,
        holder_thresholds,
        concentration_metrics,
    };
    Ok(Json(stats))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    dotenv().ok();
    let api_key = env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY must be set");
    let rpc_url = format!("https://rpc.helius.xyz/?api-key={}", api_key);
    let rpc_client = Arc::new(RpcClient::new(rpc_url));

    let cors = CorsLayer::new()
        .allow_origin("*".parse::<HeaderValue>().unwrap())
        .allow_methods([Method::GET]);

    let app = Router::new()
        .route("/token-stats", get(token_stats))
        .layer(cors)
        .with_state(rpc_client);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Listening on {}", addr);
    
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}