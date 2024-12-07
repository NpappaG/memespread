pub const MONITORED_TOKENS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS monitored_tokens (
    mint_address String,
    last_stats_update DateTime('UTC'),
    last_metrics_update DateTime('UTC'),
    created_at DateTime('UTC') DEFAULT now('UTC'),
    PRIMARY KEY (mint_address)
) ENGINE = ReplacingMergeTree
"#;

pub const TOKEN_STATS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_stats (
    mint_address String,
    timestamp DateTime('UTC') DEFAULT now('UTC'),
    price Float64,
    supply Float64,
    market_cap Float64,
    decimals UInt8,
    holders UInt32,
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = MergeTree()
"#;

pub const TOKEN_HOLDER_THRESHOLDS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_holder_thresholds (
    mint_address String,
    timestamp DateTime('UTC') DEFAULT now('UTC'),
    usd_threshold Float64,
    holder_count UInt32,
    percentage Float64,
    percentage_of_10 Float64,
    PRIMARY KEY (mint_address, timestamp, usd_threshold)
) ENGINE = MergeTree()
"#;

pub const TOKEN_CONCENTRATION_METRICS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_concentration_metrics (
    mint_address String,
    timestamp DateTime('UTC') DEFAULT now('UTC'),
    top_n UInt32,
    percentage Float64,
    PRIMARY KEY (mint_address, timestamp, top_n)
) ENGINE = MergeTree()
"#;

pub const TOKEN_DISTRIBUTION_METRICS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_distribution_metrics (
    mint_address String,
    timestamp DateTime('UTC') DEFAULT now('UTC'),
    hhi Float64,
    distribution_score Float64,
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = MergeTree()
"#;