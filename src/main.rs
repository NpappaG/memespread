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
use clickhouse::Client;
use crate::db::init::init_database;
use tokio::time::{sleep, Duration};
use poem::{handler, Route, Server, get, post, web::{Json, Data}, Response, IntoResponse};
use poem::EndpointExt;
use serde::Deserialize;
use reqwest::Client as ReqwestClient;

mod types;
mod services;
mod api;
mod db;

use crate::api::routes::create_router;
use crate::services::monitor;

async fn connect_to_clickhouse(max_retries: u32) -> Result<Client> {
    let clickhouse_url = env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
    let client = Client::default()
        .with_url(&clickhouse_url)
        .with_database("default");

    for attempt in 1..=max_retries {
        match client.query("SELECT 1").execute().await {
            Ok(_) => {
                tracing::info!("Connected to ClickHouse at {}", clickhouse_url);
                return Ok(client);
            }
            Err(e) => {
                if attempt == max_retries {
                    return Err(anyhow::anyhow!("Failed to connect to ClickHouse after {} attempts: {}", max_retries, e));
                }
                tracing::warn!("Failed to connect to ClickHouse (attempt {}/{}): {}", attempt, max_retries, e);
                sleep(Duration::from_secs(2)).await;
            }
        }
    }
    unreachable!()
}

#[derive(Deserialize)]
struct ContractInput {
    contract: String,
}

#[handler]
async fn submit(Json(input): Json<ContractInput>, client: Data<&ReqwestClient>) -> impl IntoResponse {
    // Forward the contract address to the backend API
    let url = format!("http://localhost:8000/token-stats?mint_address={}", input.contract);
    match client.get(&url).send().await {
        Ok(resp) => {
            match resp.text().await {
                Ok(body) => body,
                Err(_) => "Failed to read backend response".to_string(),
            }
        }
        Err(_) => "Failed to contact backend".to_string(),
    }
}

#[handler]
async fn index() -> impl IntoResponse {
    Response::builder()
        .content_type("text/html")
        .body(
            r#"<!DOCTYPE html>
            <html>
            <body>
                <h1>Contract Stats</h1>
                <form id="contract-form">
                    <input type="text" id="contract" placeholder="Enter contract address">
                    <button type="submit">Submit</button>
                </form>
                <div id="result"></div>
                <script>
                    document.getElementById('contract-form').addEventListener('submit', async (e) => {
                        e.preventDefault();
                        const contract = document.getElementById('contract').value;
                        const res = await fetch('/submit', {
                            method: 'POST',
                            headers: {'Content-Type': 'application/json'},
                            body: JSON.stringify({contract})
                        });
                        document.getElementById('result').innerText = await res.text();
                    });
                </script>
            </body>
            </html>"#
        )
}

#[handler]
async fn api_hello() -> &'static str {
    "Hello from API!"
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
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
    
    // Connect to ClickHouse with retries
    let client = connect_to_clickhouse(5).await?;

    // Initialize database tables
    init_database(&client).await?;

    let state = (rpc_client.clone(), rpc_limiter.clone(), client.clone());
    let app = create_router(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    tracing::info!("Listening on {}", addr);
    
    let listener = TcpListener::bind(addr).await?;

    // Start the monitoring service in a separate task
    let monitor_handle = tokio::spawn({
        let client = client.clone();
        let rpc_client = rpc_client.clone();
        let rate_limiter = rpc_limiter.clone();
        async move {
            tracing::info!("Starting monitoring service...");
            monitor::start_monitoring(client, rpc_client, rate_limiter).await;
        }
    });

    // Start the excluded accounts service in a separate task
    let excluded_accounts_handle = tokio::spawn({
        let rpc = rpc_client.clone();
        let rate_limiter = rpc_limiter.clone();
        let ch_client = client.clone();
        async move {
            tracing::info!("Starting excluded accounts service...");
            services::excluded_accounts::schedule_exclusion_updates(
                rpc,
                rate_limiter,
                ch_client,
            ).await;
        }
    });

    // Frontend server
    let client = ReqwestClient::new();
    let frontend_routes = Route::new()
        .at("/", get(index))
        .at("/submit", post(submit.data(client)));
    let frontend_server = Server::new(poem::listener::TcpListener::bind("0.0.0.0:3000")).run(frontend_routes);

    let frontend_handle = tokio::spawn(frontend_server);

    // Run both the API server and monitoring service concurrently
    tokio::select! {
        result = axum::serve(listener, app.into_make_service()) => {
            if let Err(e) = result {
                tracing::error!("Failed to serve API: {:?}", e);
            }
        }
        _ = monitor_handle => {
            tracing::info!("Monitoring service finished");
        }
        _ = excluded_accounts_handle => {
            tracing::info!("Excluded accounts service finished");
        }
        _ = frontend_handle => {
            tracing::info!("Frontend server finished");
        }
    }

    Ok(())
}