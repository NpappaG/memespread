pub const MONITORED_TOKENS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS monitored_tokens (
    mint_address String,
    last_stats_update DateTime,
    last_metrics_update DateTime,
    created_at DateTime DEFAULT now(),
    PRIMARY KEY (mint_address)
) ENGINE = ReplacingMergeTree
"#;

pub const TOKEN_STATS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_stats (
    mint_address String,
    timestamp DateTime DEFAULT now(),
    price Float64,
    supply Float64,
    market_cap Float64,
    decimals UInt8,
    holders UInt32,
    holder_thresholds String,
    concentration_metrics String,
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = MergeTree()
"#;

pub const TOKEN_DISTRIBUTION_METRICS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_distribution_metrics (
    mint_address String,
    timestamp DateTime DEFAULT now(),
    hhi Float64,
    distribution_score Float64,
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = MergeTree()
"#;