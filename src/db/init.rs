//yo
use anyhow::Result;
use clickhouse::Client;
use crate::db::schema::{
    MONITORED_TOKENS_SQL, 
    TOKEN_STATS_SQL, 
    TOKEN_DISTRIBUTION_METRICS_SQL,
    TOKEN_HOLDER_THRESHOLDS_SQL,
    TOKEN_CONCENTRATION_METRICS_SQL,
};

pub async fn init_database(client: &Client) -> Result<()> {
    tracing::info!("Initializing database tables...");
    
    // Create tables if they don't exist (won't drop existing data)
    client.query(MONITORED_TOKENS_SQL).execute().await?;
    client.query(TOKEN_STATS_SQL).execute().await?;
    client.query(TOKEN_HOLDER_THRESHOLDS_SQL).execute().await?;
    client.query(TOKEN_CONCENTRATION_METRICS_SQL).execute().await?;
    client.query(TOKEN_DISTRIBUTION_METRICS_SQL).execute().await?;

    Ok(())
}
