// This will contain our SQL schema definitions
pub const INIT_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_stats (
    id BIGSERIAL PRIMARY KEY,
    mint_address TEXT NOT NULL,
    timestamp TIMESTAMPTZ NOT NULL,
    price DOUBLE PRECISION NOT NULL,
    supply DOUBLE PRECISION NOT NULL,
    market_cap DOUBLE PRECISION NOT NULL,
    decimals INTEGER NOT NULL,
    holder_thresholds JSONB NOT NULL,
    concentration_metrics JSONB NOT NULL
);

-- Indexes for better query performance
CREATE INDEX IF NOT EXISTS idx_token_stats_mint_address 
    ON token_stats(mint_address);
CREATE INDEX IF NOT EXISTS idx_token_stats_timestamp 
    ON token_stats(timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_token_stats_mint_timestamp 
    ON token_stats(mint_address, timestamp DESC);
"#;