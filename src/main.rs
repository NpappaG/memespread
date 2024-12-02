use std::net::SocketAddr;
use anyhow::Result;
use dotenv::dotenv;
use std::env;
use std::sync::Arc;
use solana_client::nonblocking::rpc_client::RpcClient;
use tokio::net::TcpListener;
use governor::{Quota, RateLimiter};
use nonzero_ext::nonzero;
use solana_sdk::commitment_config::CommitmentConfig;

mod types;
mod services;
mod api;
mod db;

use crate::api::routes::create_router;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    
    dotenv().ok();
    let api_key = env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY must be set");
    let rpc_url = format!("https://rpc.helius.xyz/?api-key={}", api_key);
    
    let rpc_limiter = Arc::new(RateLimiter::direct(Quota::per_second(nonzero!(5u32))));
    let rpc_client = Arc::new(RpcClient::new_with_timeout_and_commitment(
        rpc_url.clone(),
        std::time::Duration::from_secs(60),
        CommitmentConfig::confirmed(),
    ));
    
    // Test RPC connection at startup
    match rpc_client.get_version().await {
        Ok(version) => tracing::info!("Connected to Solana RPC (version: {})", version.solana_core),
        Err(e) => tracing::error!("Failed to connect to RPC: {:?}", e),
    };
    
    let state = (rpc_client, rpc_limiter);
    let app = create_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Listening on {}", addr);
    
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}