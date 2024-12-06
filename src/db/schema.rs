// Split into separate table definitions
pub const TOKEN_STATS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_stats (
    timestamp DateTime,
    mint_address String,
    price Float64,
    supply Float64,
    market_cap Float64,
    decimals UInt8,
    holders UInt32,
    holder_thresholds String,
    concentration_metrics String,
    hhi Float64,
    distribution_score Float64
) ENGINE = MergeTree()
PARTITION BY toYYYYMM(timestamp)
ORDER BY (mint_address, timestamp)
"#;

pub const MONITORED_TOKENS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS monitored_tokens (
    mint_address String,
    last_stats_update DateTime,
    last_metrics_update DateTime,
    created_at DateTime DEFAULT now()
) ENGINE = ReplacingMergeTree()
ORDER BY mint_address
"#;