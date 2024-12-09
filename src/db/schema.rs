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
WITH thresholds AS (
    SELECT 10 as usd_threshold
    UNION ALL SELECT 100
    UNION ALL SELECT 1000
    UNION ALL SELECT 10000
    UNION ALL SELECT 100000
)
SELECT 
    ts.mint_address,
    thresholds.usd_threshold,
    thresholds.usd_threshold / ts.price as token_amount,
    ts.timestamp
FROM token_stats ts
CROSS JOIN thresholds
WHERE ts.price > 0
GROUP BY ts.mint_address, thresholds.usd_threshold, ts.timestamp, ts.price
"#;


pub const TOKEN_HOLDER_COUNTS_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_holder_counts_mv
TO token_holder_counts
AS 
WITH thresholds AS (
    SELECT 10 as usd_threshold
    UNION ALL SELECT 100
    UNION ALL SELECT 1000
    UNION ALL SELECT 10000
    UNION ALL SELECT 100000
),
holder_counts AS (
    SELECT 
        thb.mint_address as mint_address,
        thb.timestamp as timestamp,
        thresholds.usd_threshold as usd_threshold,
        thb.holder_address as holder_address,
        thb.balance as balance,
        tt.token_amount as min_tokens_needed,
        ts.decimals as decimals
    FROM token_holder_balances thb
    CROSS JOIN thresholds
    JOIN token_thresholds tt 
        ON thb.mint_address = tt.mint_address 
        AND thb.timestamp = tt.timestamp
        AND tt.usd_threshold = thresholds.usd_threshold
    JOIN token_stats ts
        ON thb.mint_address = ts.mint_address
        AND thb.timestamp = ts.timestamp
)
SELECT 
    holder_counts.mint_address,
    holder_counts.timestamp,
    holder_counts.usd_threshold,
    count(DISTINCT CASE WHEN holder_counts.balance / pow(10, holder_counts.decimals) >= holder_counts.min_tokens_needed 
                       THEN holder_counts.holder_address END) as holder_count
FROM holder_counts
GROUP BY 
    holder_counts.mint_address, 
    holder_counts.timestamp, 
    holder_counts.usd_threshold
"#;

pub const TOKEN_CONCENTRATION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW token_concentration_mv
TO token_concentration
AS SELECT
    thb.mint_address as mint_address, 
    toDateTime(ts.timestamp, 'UTC') as timestamp,
    t.top_n as top_n,
    sum(thb.balance) / any(ts.supply) * 100 as percentage
FROM (
    SELECT 
        mint_address,
        holder_address,
        balance,
        toDateTime(timestamp, 'UTC') as timestamp,
        row_number() OVER (PARTITION BY mint_address, timestamp ORDER BY balance DESC) as holder_rank
    FROM token_holder_balances
) thb
CROSS JOIN (
    SELECT 1 AS top_n
    UNION ALL SELECT 10
    UNION ALL SELECT 25
    UNION ALL SELECT 50
    UNION ALL SELECT 100
    UNION ALL SELECT 250
) t
JOIN token_stats ts ON thb.mint_address = ts.mint_address AND thb.timestamp = ts.timestamp
WHERE holder_rank <= t.top_n
GROUP BY thb.mint_address, ts.timestamp, t.top_n;
"#;

pub const TOKEN_DISTRIBUTION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_distribution_mv
TO token_distribution
AS 
WITH all_metrics AS (
    SELECT 
        thb.mint_address as mint_address,
        thb.timestamp as timestamp,
        thb.balance as balance,
        ts.supply as supply,
        tt.token_amount as threshold_amount
    FROM token_holder_balances thb
    JOIN token_stats ts ON thb.mint_address = ts.mint_address AND thb.timestamp = ts.timestamp
    JOIN token_thresholds tt ON thb.mint_address = tt.mint_address 
        AND thb.timestamp = tt.timestamp
        AND tt.usd_threshold = 10
)
SELECT 
    all_metrics.mint_address,
    all_metrics.timestamp,
    -- All holders metrics
    sum(pow((all_metrics.balance / all_metrics.supply) * 100, 2)) as hhi,
    1 - (sum(pow((all_metrics.balance / all_metrics.supply), 2)) / pow(sum(all_metrics.balance / all_metrics.supply), 2)) as distribution_score,
    -- Holders above $10 metrics
    sum(pow((CASE WHEN all_metrics.balance >= all_metrics.threshold_amount THEN all_metrics.balance ELSE 0 END) / all_metrics.supply * 100, 2)) as hhi_10usd,
    1 - (
        sum(pow((CASE WHEN all_metrics.balance >= all_metrics.threshold_amount THEN all_metrics.balance ELSE 0 END) / all_metrics.supply, 2)) / 
        pow(sum(CASE WHEN all_metrics.balance >= all_metrics.threshold_amount THEN all_metrics.balance / all_metrics.supply ELSE 0 END), 2)
    ) as distribution_score_10usd
FROM all_metrics
GROUP BY all_metrics.mint_address, all_metrics.timestamp
"#;
