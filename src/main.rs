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
use poem::{handler, Route, Server, get, post, web::{Json, Path}, Response, IntoResponse, middleware::Cors, EndpointExt};
use serde::{Deserialize, Serialize};

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

#[derive(Deserialize, Serialize)]
struct ContractInput {
    contract: String,
}

#[handler]
async fn submit(Json(input): Json<ContractInput>) -> impl IntoResponse {
    Response::builder()
        .content_type("application/json")
        .body(format!("{{\"status\": \"success\", \"message\": \"Token {} submitted for monitoring\"}}", input.contract))
}

#[handler]
async fn token_details(Path(mint_address): Path<String>) -> impl IntoResponse {
    Response::builder()
        .content_type("text/html")
        .body(
            format!(r#"<!DOCTYPE html>
            <html>
            <head>
                <style>
                    body {{ 
                        font-family: Arial, sans-serif; 
                        max-width: 1200px; 
                        margin: 40px auto; 
                        padding: 0 20px;
                        background: #f9f9f9;
                    }}
                    h1, h2 {{ color: #333; }}
                    .back-link {{ 
                        display: inline-block;
                        margin-bottom: 20px;
                        color: #666;
                        text-decoration: none;
                    }}
                    .back-link:hover {{ color: #333; }}
                    .loading {{ 
                        color: #666;
                        font-style: italic;
                        text-align: center;
                        padding: 40px;
                    }}
                    .error {{ color: #d32f2f; }}
                    
                    /* Hero Sections */
                    .hero-section {{
                        background: white;
                        border-radius: 12px;
                        padding: 24px;
                        margin: 20px 0;
                        box-shadow: 0 2px 4px rgba(0,0,0,0.1);
                    }}
                    
                    /* Token Stats */
                    .token-stats {{
                        display: grid;
                        grid-template-columns: repeat(3, 1fr);
                        gap: 20px;
                        text-align: center;
                    }}
                    .market-cap {{
                        grid-column: 1 / -1;
                        font-size: 2em;
                        padding: 20px;
                        background: #f8f9fa;
                        border-radius: 8px;
                        margin-bottom: 20px;
                    }}
                    .stat-card {{
                        padding: 15px;
                        background: #f8f9fa;
                        border-radius: 8px;
                    }}
                    .stat-label {{
                        color: #666;
                        font-size: 0.9em;
                        margin-bottom: 5px;
                    }}
                    .stat-value {{
                        font-size: 1.2em;
                        font-weight: bold;
                    }}
                    
                    /* Holder Thresholds */
                    .toggle-container {{
                        text-align: center;
                        margin: 20px 0;
                    }}
                    .toggle-btn {{
                        background: #fff;
                        border: 1px solid #ddd;
                        padding: 8px 16px;
                        border-radius: 20px;
                        cursor: pointer;
                        margin: 0 5px;
                    }}
                    .toggle-btn.active {{
                        background: #007bff;
                        color: white;
                        border-color: #007bff;
                    }}
                    .threshold-bar {{
                        margin: 15px 0;
                        display: flex;
                        align-items: center;
                    }}
                    .threshold-label {{
                        width: 80px;
                        font-weight: bold;
                    }}
                    .bar-container {{
                        flex-grow: 1;
                        height: 24px;
                        background: #eee;
                        border-radius: 12px;
                        margin: 0 15px;
                        overflow: hidden;
                    }}
                    .bar {{
                        height: 100%;
                        background: #007bff;
                        transition: width 0.3s ease;
                    }}
                    .threshold-value {{
                        width: 200px;
                        text-align: right;
                    }}
                    
                    /* Layout */
                    .metrics-layout {{
                        display: flex;
                        gap: 20px;
                        margin: 20px 0;
                    }}
                    .metrics-layout > div {{
                        flex: 1;
                    }}
                    @media (max-width: 768px) {{
                        .metrics-layout {{
                            flex-direction: column;
                        }}
                    }}
                    
                    /* Metrics Grid */
                    .metrics-grid {{
                        display: grid;
                        grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
                        gap: 20px;
                    }}
                    .metric-card {{
                        background: white;
                        padding: 15px;
                        border-radius: 8px;
                        box-shadow: 0 1px 3px rgba(0,0,0,0.1);
                    }}
                    .metric-label {{
                        color: #666;
                        font-size: 0.9em;
                        margin-bottom: 5px;
                    }}
                    .metric-value {{
                        font-size: 1.1em;
                        font-weight: bold;
                    }}
                </style>
            </head>
            <body>
                <a href="/" class="back-link">&larr; Back to Token List</a>
                <h1>Token Details</h1>
                <div class="token-address">{}</div>
                <div id="content" class="loading">Loading token details...</div>
                
                <script>
                    function formatNumber(num, decimals = 2) {{
                        const absNum = Math.abs(num);
                        if (absNum >= 1e9) {{
                            return (num / 1e9).toFixed(decimals) + 'B';
                        }} else if (absNum >= 1e6) {{
                            return (num / 1e6).toFixed(decimals) + 'M';
                        }} else if (absNum >= 1e3) {{
                            return (num / 1e3).toFixed(decimals) + 'K';
                        }}
                        return absNum < 1 ? num.toFixed(6) : num.toFixed(decimals);
                    }}

                    function updateHolderThresholds(data, useTotal = true) {{
                        const thresholds = data.holder_thresholds;
                        const container = document.getElementById('holder-thresholds');
                        container.innerHTML = '';
                        
                        thresholds.forEach(t => {{
                            const percentage = useTotal ? t.pct_total_holders : t.pct_of_10usd;
                            const barWidth = useTotal ? (t.holder_count / data.holder_thresholds[0].total_holders) * 100 : percentage;
                            
                            const bar = document.createElement('div');
                            bar.className = 'threshold-bar';
                            bar.innerHTML = `
                                <div class="threshold-label">>${{formatNumber(t.usd_threshold, 0)}}</div>
                                <div class="bar-container">
                                    <div class="bar" style="width: ${{barWidth}}%"></div>
                                </div>
                                                                    <div class="threshold-value">
                                     ${{formatNumber(t.holder_count, 0)}} 
                                     (${{percentage.toFixed(1)}}% of ${{useTotal ? 'total' : '>$10'}})
                                </div>
                            `;
                            container.appendChild(bar);
                        }});
                    }}

                                        function updateConcentrationBars(data) {{
                        const bars = document.querySelectorAll('#concentration-bars .threshold-bar');
                        if (!bars.length) return;

                        // Update existing bars with concentration metrics
                        data.concentration_metrics.forEach((m, i) => {{
                            if (bars[i]) {{
                                const bar = bars[i].querySelector('.bar');
                                const value = bars[i].querySelector('.threshold-value');
                                if (bar) bar.style.width = m.percentage + '%';
                                if (value) value.textContent = m.percentage.toFixed(2) + '%';
                            }}
                        }});

                        // Update the remaining holders bar (last bar)
                        const lastBar = bars[bars.length - 1];
                        if (lastBar && data.concentration_metrics.length > 0) {{
                            const lastMetric = data.concentration_metrics[data.concentration_metrics.length - 1];
                            const remainingPercentage = 100 - lastMetric.percentage;
                            const bar = lastBar.querySelector('.bar');
                            const value = lastBar.querySelector('.threshold-value');
                            if (bar) bar.style.width = remainingPercentage + '%';
                            if (value) value.textContent = remainingPercentage.toFixed(2) + '%';
                        }}
                    }}

                    async function loadTokenDetails() {{
                        const contentDiv = document.getElementById('content');
                        try {{
                            const res = await fetch('http://localhost:8000/tokens/' + encodeURIComponent('{}'));
                            if (!res.ok) throw new Error(`HTTP error! status: ${{res.status}}`);
                            const data = await res.text();
                            try {{
                                const jsonData = JSON.parse(data);
                                contentDiv.className = '';
                                contentDiv.innerHTML = `
                                    <!-- Token Stats Hero -->
                                    <div class="hero-section">
                                        <div class="token-stats">
                                            <div class="market-cap">
                                                <div class="stat-label">Market Cap</div>
                                                <div class="stat-value">$${{formatNumber(jsonData.token_stats.price * (jsonData.token_stats.supply / Math.pow(10, jsonData.token_stats.decimals)))}}</div>
                                            </div>
                                            <div class="stat-card">
                                                <div class="stat-label">Price</div>
                                                <div class="stat-value">$${{jsonData.token_stats.price}}</div>
                                            </div>
                                            <div class="stat-card">
                                                <div class="stat-label">Supply</div>
                                                <div class="stat-value">${{formatNumber(jsonData.token_stats.supply / Math.pow(10, jsonData.token_stats.decimals))}}</div>
                                            </div>
                                        </div>
                                    </div>

                                    <div class="metrics-layout">
                                        <!-- Holder Thresholds Hero -->
                                        <div class="hero-section">
                                            <h2>Holder Thresholds</h2>
                                            <div class="toggle-container">
                                                <button class="toggle-btn active" onclick="this.classList.add('active'); this.nextElementSibling.classList.remove('active'); updateHolderThresholds(window.tokenData, true)">All Wallets</button>
                                                <button class="toggle-btn" onclick="this.classList.add('active'); this.previousElementSibling.classList.remove('active'); updateHolderThresholds(window.tokenData, false)">>$10 Wallets</button>
                                            </div>
                                            <div id="holder-thresholds"></div>
                                        </div>

                                        <!-- Concentration Metrics Hero -->
                                        <div class="hero-section">
                                            <h2>Concentration Metrics</h2>
                                            <div id="concentration-bars">
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">#1 Top Holder</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">Top 10 Holders</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">Top 25 Holders</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">Top 50 Holders</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">Top 100 Holders</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">Top 250 Holders</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                                <div class="threshold-bar">
                                                    <div class="threshold-label">251+ Holders &infin;</div>
                                                    <div class="bar-container"><div class="bar"></div></div>
                                                    <div class="threshold-value">0%</div>
                                                </div>
                                            </div>
                                        </div>
                                    </div>

                                    <!-- Distribution Stats Hero -->
                                    <div class="hero-section">
                                        <h2>Distribution Stats</h2>
                                        <div class="metrics-grid">
                                            <div class="metric-card">
                                                <div class="metric-label">Distribution Score</div>
                                                <div class="metric-value">${{jsonData.distribution_stats.distribution_score.toFixed(2)}}</div>
                                            </div>
                                            <div class="metric-card">
                                                <div class="metric-label">HHI</div>
                                                <div class="metric-value">${{jsonData.distribution_stats.hhi.toFixed(2)}}</div>
                                            </div>
                                            <div class="metric-card">
                                                <div class="metric-label">Mean Balance</div>
                                                <div class="metric-value">${{formatNumber(jsonData.distribution_stats.mean_balance)}}</div>
                                            </div>
                                            <div class="metric-card">
                                                <div class="metric-label">Median Balance</div>
                                                <div class="metric-value">${{formatNumber(jsonData.distribution_stats.median_balance)}}</div>
                                            </div>
                                        </div>
                                    </div>
                                `;
                                
                                                                                // Store data globally for the toggle functionality
                                                window.tokenData = jsonData;
                                                updateHolderThresholds(jsonData, true);
                                                updateConcentrationBars(jsonData);
                                
                            }} catch (parseError) {{
                                contentDiv.className = 'error';
                                contentDiv.textContent = data;
                            }}
                        }} catch (error) {{
                            contentDiv.className = 'error';
                            contentDiv.textContent = 'Error loading token details: ' + error.message;
                        }}
                    }}
                    loadTokenDetails();
                </script>
            </body>
            </html>"#,
            mint_address, mint_address
        )
    )
}

#[handler]
async fn index() -> impl IntoResponse {
    Response::builder()
        .content_type("text/html")
        .body(
            r#"<!DOCTYPE html>
            <html>
            <head>
                <style>
                    body { font-family: Arial, sans-serif; max-width: 800px; margin: 40px auto; padding: 0 20px; }
                    h1, h2 { color: #333; }
                    .token-list {
                        margin-top: 30px;
                    }
                    .token-item {
                        padding: 15px;
                        border: 1px solid #ddd;
                        border-radius: 4px;
                        margin: 10px 0;
                        cursor: pointer;
                        transition: all 0.2s;
                        text-decoration: none;
                        color: inherit;
                        display: block;
                    }
                    .token-item:hover {
                        background-color: #f5f5f5;
                        transform: translateX(5px);
                    }
                    .token-address {
                        color: #2196F3;
                        font-weight: bold;
                        font-family: monospace;
                    }
                    .token-time {
                        color: #666;
                        font-size: 0.9em;
                        margin-top: 5px;
                    }
                    .loading {
                        color: #666;
                        font-style: italic;
                    }
                    .error {
                        color: #d32f2f;
                        padding: 10px;
                        border: 1px solid #ffcdd2;
                        border-radius: 4px;
                        background: #ffebee;
                    }
                    #add-token {
                        margin-top: 30px;
                        padding: 20px;
                        background: #f5f5f5;
                        border-radius: 4px;
                    }
                    #add-token h2 {
                        margin-top: 0;
                    }
                    input[type="text"] { 
                        padding: 8px; 
                        width: 300px; 
                        margin-right: 10px;
                        border: 1px solid #ddd;
                        border-radius: 4px;
                    }
                    button { 
                        padding: 8px 16px; 
                        background: #4CAF50; 
                        color: white; 
                        border: none;
                        border-radius: 4px;
                        cursor: pointer;
                    }
                    button:hover { background: #45a049; }
                </style>
            </head>
            <body>
                <h1>Token Stats Monitor</h1>
                <div id="token-list" class="token-list loading">Loading monitored tokens...</div>

                <div id="add-token">
                    <h2>Monitor New Token</h2>
                    <form id="contract-form">
                        <input type="text" id="contract" placeholder="Enter token mint address">
                        <button type="submit">Monitor Token</button>
                    </form>
                    <div id="result"></div>
                </div>

                <script>
                    // Format date string to relative time
                    function timeAgo(dateStr) {
                        const date = new Date(dateStr);
                        const now = new Date();
                        const seconds = Math.floor((now - date) / 1000);
                        
                        let interval = Math.floor(seconds / 31536000);
                        if (interval > 1) return interval + ' years ago';
                        if (interval === 1) return 'a year ago';
                        
                        interval = Math.floor(seconds / 2592000);
                        if (interval > 1) return interval + ' months ago';
                        if (interval === 1) return 'a month ago';
                        
                        interval = Math.floor(seconds / 86400);
                        if (interval > 1) return interval + ' days ago';
                        if (interval === 1) return 'yesterday';
                        
                        interval = Math.floor(seconds / 3600);
                        if (interval > 1) return interval + ' hours ago';
                        if (interval === 1) return 'an hour ago';
                        
                        interval = Math.floor(seconds / 60);
                        if (interval > 1) return interval + ' minutes ago';
                        if (interval === 1) return 'a minute ago';
                        
                        if (seconds < 10) return 'just now';
                        
                        return Math.floor(seconds) + ' seconds ago';
                    }

                    // Load and display token list
                    async function loadTokenList() {
                        const listDiv = document.getElementById('token-list');
                        try {
                            const res = await fetch('http://localhost:8000/tokens');
                            if (!res.ok) throw new Error(`HTTP error! status: ${res.status}`);
                            const tokens = await res.json();
                            
                            if (tokens.length === 0) {
                                listDiv.className = 'token-list';
                                listDiv.innerHTML = '<div class="token-item" style="cursor: default; background: #f9f9f9;">No tokens monitored yet. Add your first token below!</div>';
                                return;
                            }

                            const tokenListHtml = tokens.map(token => `
                                <a href="/token/${token.mint_address}" class="token-item">
                                    <div class="token-address">${token.mint_address}</div>
                                    <div class="token-time">
                                        Last updated: ${timeAgo(token.last_stats_update)}
                                    </div>
                                </a>
                            `).join('');
                            
                            listDiv.className = 'token-list';
                            listDiv.innerHTML = tokenListHtml;
                        } catch (error) {
                            listDiv.className = 'token-list error';
                            listDiv.textContent = 'Error loading token list: ' + error.message;
                        }
                    }

                    // Handle new token submission
                    document.getElementById('contract-form').addEventListener('submit', async (e) => {
                        e.preventDefault();
                        const contract = document.getElementById('contract').value;
                        const resultDiv = document.getElementById('result');
                        resultDiv.className = 'loading';
                        resultDiv.textContent = 'Processing...';
                        
                        try {
                            const res = await fetch('http://localhost:8000/tokens', {
                                method: 'POST',
                                headers: {'Content-Type': 'application/json'},
                                body: JSON.stringify({mint_address: contract})
                            });
                            if (!res.ok) throw new Error(`HTTP error! status: ${res.status}`);
                            
                            const data = await res.text();
                            try {
                                const jsonData = JSON.parse(data);
                                resultDiv.className = '';
                                resultDiv.textContent = JSON.stringify(jsonData, null, 2);
                                // Clear input on success
                                document.getElementById('contract').value = '';
                                // Refresh token list
                                loadTokenList();
                            } catch {
                                resultDiv.className = 'error';
                                resultDiv.textContent = data;
                            }
                        } catch (error) {
                            resultDiv.className = 'error';
                            resultDiv.textContent = 'Error: ' + error.message;
                        }
                    });

                    // Initial load of token list
                    loadTokenList();
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
    let app = create_router(state.clone());

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

    // Frontend routes
    let frontend_routes = Route::new()
        .at("/", get(index))
        .at("/token/:mint_address", get(token_details))
        .at("/tokens", post(submit))
        .with(Cors::new()
            .allow_origin_regex(".*")  // Allow all origins in development
            .allow_methods(vec!["GET", "POST"])
            .allow_headers(vec!["Content-Type"]));
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