use anyhow::Result;
use clickhouse::Client;
use crate::db::schema::{
    MONITORED_TOKENS_SQL,
    TOKEN_STATS_SQL,
    TOKEN_HOLDERS_SQL,
    EXCLUDED_ACCOUNTS_SQL,
    // Target tables
    TOKEN_HOLDER_BALANCES_TABLE_SQL,
    TOKEN_THRESHOLDS_TABLE_SQL,
    TOKEN_HOLDER_COUNTS_TABLE_SQL,
    TOKEN_CONCENTRATION_TABLE_SQL,
    TOKEN_DISTRIBUTION_TABLE_SQL,
    // Materialized views
    TOKEN_HOLDER_BALANCES_MV_SQL,
    TOKEN_THRESHOLDS_MV_SQL,
    TOKEN_HOLDER_COUNTS_MV_SQL,
    TOKEN_CONCENTRATION_MV_SQL,
    TOKEN_DISTRIBUTION_MV_SQL,
};

pub async fn init_database(client: &Client) -> Result<()> {
    tracing::info!("Starting database initialization...");
    
    // First verify we can execute queries
    match client.query("SELECT currentDatabase() as db").fetch_one::<String>().await {
        Ok(db) => tracing::info!("Connected to database: {}", db),
        Err(e) => {
            tracing::error!("Failed to verify database connection: {}", e);
            return Err(e.into());
        }
    }
    
    tracing::info!("Creating base tables...");
    
    // Base tables first
    for sql in [
        MONITORED_TOKENS_SQL,
        TOKEN_STATS_SQL,
        TOKEN_HOLDERS_SQL,
        EXCLUDED_ACCOUNTS_SQL,
    ] {
        match client.query(sql).execute().await {
            Ok(_) => tracing::info!("Successfully created/verified table from SQL: {}", &sql[..100]),
            Err(e) => {
                tracing::error!("Failed to create base table. Error: {}", e);
                tracing::error!("Failed SQL: {}", sql);
                return Err(e.into());
            }
        }
    }

    tracing::info!("Creating target tables for materialized views...");
    
    // Create target tables before MVs
    for sql in [
        TOKEN_HOLDER_BALANCES_TABLE_SQL,
        TOKEN_THRESHOLDS_TABLE_SQL,
        TOKEN_HOLDER_COUNTS_TABLE_SQL,
        TOKEN_CONCENTRATION_TABLE_SQL,
        TOKEN_DISTRIBUTION_TABLE_SQL,
    ] {
        match client.query(sql).execute().await {
            Ok(_) => tracing::info!("Successfully created/verified target table from SQL: {}", &sql[..100]),
            Err(e) => {
                tracing::error!("Failed to create target table. Error: {}", e);
                tracing::error!("Failed SQL: {}", sql);
                return Err(e.into());
            }
        }
    }

    tracing::info!("Creating materialized views...");
    
    // MVs in dependency order with verification
    let mv_configs = [
        ("token_holder_balances_mv", TOKEN_HOLDER_BALANCES_MV_SQL),
        ("token_thresholds_mv", TOKEN_THRESHOLDS_MV_SQL),
        ("token_holder_counts_mv", TOKEN_HOLDER_COUNTS_MV_SQL),
        ("token_concentration_mv", TOKEN_CONCENTRATION_MV_SQL),
        ("token_distribution_mv", TOKEN_DISTRIBUTION_MV_SQL),
    ];

    for (name, sql) in mv_configs {
        match client.query(sql).execute().await {
            Ok(_) => {
                tracing::info!("Successfully created/verified materialized view: {}", name);
            }
            Err(e) => {
                if e.to_string().contains("already exists") {
                    tracing::info!("Materialized view {} already exists, skipping", name);
                    continue;
                }
                tracing::error!("Failed to create materialized view {}. Error: {}", name, e);
                tracing::error!("Failed SQL: {}", sql);
                return Err(e.into());
            }
        }
    }

    tracing::info!("Database initialization completed successfully!");
    Ok(())
}
