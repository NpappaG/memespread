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
    timestamp DateTime('UTC'),
    price Float64,
    supply Float64,
    market_cap Float64,
    decimals UInt8,
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = ReplacingMergeTree
"#;

// Raw holder data from every rpc call
pub const TOKEN_HOLDERS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_holders (
    mint_address String,
    token_account String,
    holder_address String,
    amount UInt64,
    timestamp DateTime('UTC'),
    PRIMARY KEY (mint_address, holder_address, timestamp)
) ENGINE = ReplacingMergeTree
"#;

//accumulated exclusions list checked every 24 hrs
pub const EXCLUDED_ACCOUNTS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS excluded_accounts (
    address String,
    category String,
    description String,
    added_at DateTime('UTC') DEFAULT now('UTC'),
    PRIMARY KEY (address)
) ENGINE = ReplacingMergeTree
"#;

// Target tables for MVs
//this is the cleaned up holder data - removing the exclusions
pub const TOKEN_HOLDER_BALANCES_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_holder_balances (
    mint_address String,
    holder_address String,
    balance Float64,
    timestamp DateTime('UTC'),
    PRIMARY KEY (mint_address, holder_address)
) ENGINE = ReplacingMergeTree
"#;


pub const TOKEN_THRESHOLDS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_thresholds (
    mint_address String,
    usd_threshold Float64,
    token_amount Float64,
    timestamp DateTime('UTC'),
    PRIMARY KEY (mint_address, usd_threshold, timestamp)
) ENGINE = ReplacingMergeTree
"#;


pub const TOKEN_HOLDER_COUNTS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_holder_counts (
    mint_address String,
    timestamp DateTime('UTC'),
    usd_threshold Float64,
    holder_count UInt64,
    total_holders UInt64,
    pct_total_holders Float64 DEFAULT 0,
    pct_of_10usd Float64 DEFAULT 0,
    mcap_per_holder Float64 DEFAULT 0,
    slice_value_usd Float64 DEFAULT 0,
    PRIMARY KEY (mint_address, timestamp, usd_threshold)
) ENGINE = ReplacingMergeTree
"#;

pub const TOKEN_CONCENTRATION_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_concentration (
    mint_address String,
    timestamp DateTime('UTC'),
    top_n UInt8,
    percentage Float64,
    PRIMARY KEY (mint_address, timestamp, top_n)
) ENGINE = ReplacingMergeTree
"#;

pub const TOKEN_DISTRIBUTION_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_distribution (
    mint_address String,
    timestamp DateTime('UTC'),
    hhi Float64,
    hhi_10usd Float64,
    distribution_score Float64,
    distribution_score_10usd Float64,
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = ReplacingMergeTree
"#;

// Materialized Views in dependency order
pub const TOKEN_HOLDER_BALANCES_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_holder_balances_mv
TO token_holder_balances
AS SELECT
    th.mint_address,
    th.holder_address,
    toFloat64(sum(th.amount)) as balance,
    th.timestamp
FROM token_holders th
LEFT ANTI JOIN excluded_accounts ea ON th.holder_address = ea.address
GROUP BY th.mint_address, th.holder_address, th.timestamp
"#;

pub const TOKEN_THRESHOLDS_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_thresholds_mv
TO token_thresholds
AS 
SELECT 
    ts.mint_address,
    value as usd_threshold,
    value / ts.price as token_amount,
    ts.timestamp
FROM token_stats ts
INNER JOIN token_holder_balances thb 
    ON ts.mint_address = thb.mint_address 
    AND ts.timestamp = thb.timestamp
ARRAY JOIN [10, 100, 1000, 10000, 100000] as value
WHERE ts.price > 0
"#;


pub const TOKEN_HOLDER_COUNTS_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_holder_counts_mv
TO token_holder_counts
AS 
SELECT 
    thb.mint_address as mint_address,
    thb.timestamp as timestamp,
    tt.usd_threshold,
    countDistinct(multiIf(thb.balance / pow(10, ts.decimals) >= tt.token_amount, thb.holder_address, NULL)) AS holder_count,
    countDistinct(thb.holder_address) AS total_holders,
    coalesce((holder_count / nullIf(total_holders, 0)) * 100, 0) AS pct_total_holders,
    coalesce((holder_count / nullIf(any(holder_count) OVER (PARTITION BY thb.mint_address, thb.timestamp ORDER BY tt.usd_threshold ASC), 0)) * 100, 0) AS pct_of_10usd,
    coalesce(max(ts.market_cap) / nullIf(holder_count, 0), 0) AS mcap_per_holder,
    coalesce(sum(multiIf(thb.balance / pow(10, ts.decimals) >= tt.token_amount, thb.balance / pow(10, ts.decimals) * ts.price, 0)), 0) AS slice_value_usd
FROM token_holder_balances thb
JOIN (
    SELECT mint_address, max(timestamp) as max_ts
    FROM token_stats
    GROUP BY mint_address
) latest_ts ON thb.mint_address = latest_ts.mint_address
JOIN token_stats ts 
    ON thb.mint_address = ts.mint_address 
    AND ts.timestamp = latest_ts.max_ts
JOIN token_thresholds tt 
    ON thb.mint_address = tt.mint_address 
    AND tt.timestamp = latest_ts.max_ts
WHERE tt.usd_threshold IN (10, 100, 1000, 10000, 100000)
GROUP BY
    thb.mint_address,
    thb.timestamp,
    tt.usd_threshold
"#;

pub const TOKEN_CONCENTRATION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_concentration_mv
TO token_concentration
AS WITH ranked_holders AS
(
    SELECT
        thb.mint_address,
        thb.timestamp,
        thb.balance,
        row_number() OVER (PARTITION BY thb.mint_address, thb.timestamp ORDER BY thb.balance DESC) AS rank
    FROM token_holder_balances AS thb
)
SELECT
    rh.mint_address,
    rh.timestamp,
    t.top_n,
    (sum(rh.balance) / max(ts.supply)) * 100 AS percentage
FROM ranked_holders AS rh
INNER JOIN token_stats AS ts ON (rh.mint_address = ts.mint_address) AND (rh.timestamp = ts.timestamp)
CROSS JOIN
(
    SELECT 1 AS top_n
    UNION ALL SELECT 10
    UNION ALL SELECT 25
    UNION ALL SELECT 50
    UNION ALL SELECT 100
    UNION ALL SELECT 250
) AS t
WHERE rh.rank <= t.top_n
GROUP BY
    rh.mint_address,
    rh.timestamp,
    t.top_n
"#;

pub const TOKEN_DISTRIBUTION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_distribution_mv
TO token_distribution
AS SELECT
    thb.mint_address,
    thb.timestamp,
    sum(pow((thb.balance / ts.supply) * 100, 2)) AS hhi,
    1 - (sum(pow(thb.balance / ts.supply, 2)) / pow(sum(thb.balance / ts.supply), 2)) AS distribution_score,
    sum(pow(multiIf((thb.balance / pow(10, ts.decimals)) >= tt.token_amount, thb.balance / ts.supply, 0) * 100, 2)) AS hhi_10usd,
    1 - (sum(pow(multiIf((thb.balance / pow(10, ts.decimals)) >= tt.token_amount, thb.balance / ts.supply, 0), 2)) / pow(sum(multiIf((thb.balance / pow(10, ts.decimals)) >= tt.token_amount, thb.balance / ts.supply, 0)), 2)) AS distribution_score_10usd
FROM token_holder_balances AS thb
INNER JOIN token_stats AS ts ON (thb.mint_address = ts.mint_address) AND (thb.timestamp = ts.timestamp)
INNER JOIN token_thresholds AS tt ON (thb.mint_address = tt.mint_address) AND (thb.timestamp = tt.timestamp) AND (tt.usd_threshold = 10)
GROUP BY
    thb.mint_address,
    thb.timestamp
"#;
