use anyhow::Result;
use clickhouse::Client;
use crate::db::schema::{
    MONITORED_TOKENS_SQL,
    TOKEN_STATS_SQL,
    TOKEN_HOLDERS_SQL,
    EXCLUDED_ACCOUNTS_SQL,
    // Target tables
    TOKEN_HOLDER_BALANCES_TABLE_SQL,
    TOKEN_HOLDER_COUNTS_TABLE_SQL,
    TOKEN_CONCENTRATION_TABLE_SQL,
    TOKEN_DISTRIBUTION_TABLE_SQL,
    // Materialized views
    TOKEN_HOLDER_BALANCES_MV_SQL,
    TOKEN_HOLDER_COUNTS_MV_SQL,
    TOKEN_CONCENTRATION_MV_SQL,
    TOKEN_DISTRIBUTION_MV_SQL,
};

pub async fn init_database(client: &Client) -> Result<()> {
    tracing::info!("Initializing database tables...");
    
    // Base tables first
    for sql in [
        MONITORED_TOKENS_SQL,
        TOKEN_STATS_SQL,
        TOKEN_HOLDERS_SQL,
        EXCLUDED_ACCOUNTS_SQL,
    ] {
        if let Err(e) = client.query(sql).execute().await {
            tracing::error!("Failed to create base table: {}", e);
            return Err(e.into());
        }
    }

    tracing::info!("Initializing target tables for materialized views...");
    
    // Create target tables before MVs
    for sql in [
        TOKEN_HOLDER_BALANCES_TABLE_SQL,
        TOKEN_HOLDER_COUNTS_TABLE_SQL,
        TOKEN_CONCENTRATION_TABLE_SQL,
        TOKEN_DISTRIBUTION_TABLE_SQL,
    ] {
        if let Err(e) = client.query(sql).execute().await {
            tracing::error!("Failed to create target table: {}", e);
            return Err(e.into());
        }
    }

    tracing::info!("Initializing materialized views...");
    
    // MVs in dependency order with verification
    let mv_configs = [
        ("token_holder_balances_mv", TOKEN_HOLDER_BALANCES_MV_SQL),
        ("token_holder_counts_mv", TOKEN_HOLDER_COUNTS_MV_SQL),
        ("token_concentration_mv", TOKEN_CONCENTRATION_MV_SQL),
        ("token_distribution_mv", TOKEN_DISTRIBUTION_MV_SQL),
    ];

    for (name, sql) in mv_configs {
        match client.query(sql).execute().await {
            Ok(_) => {
                // Verify MV exists
                let status = client
                    .query("SELECT engine FROM system.tables WHERE name = ?")
                    .bind(name)
                    .fetch_one::<String>()
                    .await;
                
                match status {
                    Ok(engine) => {
                        tracing::info!("Created MV {} (engine: {})", name, engine);
                    }
                    Err(e) => {
                        tracing::error!("Failed to verify MV {}: {}", name, e);
                        return Err(e.into());
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to create MV {}: {}", name, e);
                return Err(e.into());
            }
        }
    }

    Ok(())
}
