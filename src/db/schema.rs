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

pub const TOKEN_HOLDERS_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_holders (
    mint_address String,
    token_account String,
    holder_address String,
    amount UInt64,
    timestamp DateTime('UTC') DEFAULT now('UTC'),
    PRIMARY KEY (mint_address, holder_address, timestamp)
) ENGINE = ReplacingMergeTree
"#;

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
pub const TOKEN_HOLDER_BALANCES_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS token_holder_balances (
    mint_address String,
    holder_address String,
    balance Float64,
    timestamp DateTime('UTC'),
    PRIMARY KEY (mint_address, holder_address)
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
    denominator UInt64,
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
    toFloat64(sum(th.amount)) as balance
FROM token_holders th
LEFT JOIN excluded_accounts ea ON ea.address = th.holder_address
WHERE ea.address IS NULL
GROUP BY th.mint_address, th.holder_address
"#;

pub const TOKEN_HOLDER_COUNTS_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_holder_counts_mv
TO token_holder_counts
AS SELECT 
    thb.mint_address,
    ts.timestamp,
    mv.threshold as usd_threshold,
    uniqState(thb.holder_address) as holder_count
FROM (
    SELECT 
        mint_address,
        holder_address,
        sumMerge(total_amount) as balance
    FROM token_holder_balances_mv
    GROUP BY mint_address, holder_address
) thb
CROSS JOIN (
    SELECT 0 AS threshold
    UNION ALL SELECT 10
    UNION ALL SELECT 100
    UNION ALL SELECT 1000
    UNION ALL SELECT 10000
    UNION ALL SELECT 100000
    UNION ALL SELECT 1000000
) mv
JOIN token_stats ts ON thb.mint_address = ts.mint_address
WHERE thb.balance * ts.price >= mv.threshold
GROUP BY thb.mint_address, ts.timestamp, mv.threshold
"#;

pub const TOKEN_CONCENTRATION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_concentration_mv
TO token_concentration
AS SELECT
    holders.mint_address,
    ts.timestamp,
    t.top_n,
    sum(holders.amount) / any(ts.supply) * 100 as percentage
FROM (
    SELECT 
        mint_address,
        holder_address,
        sumMerge(total_amount) as amount,
        row_number() OVER (PARTITION BY mint_address ORDER BY amount DESC) as holder_rank
    FROM token_holder_balances_mv
    GROUP BY mint_address, holder_address
) holders
CROSS JOIN (
    SELECT 1 AS top_n
    UNION ALL SELECT 10
    UNION ALL SELECT 25
    UNION ALL SELECT 50
    UNION ALL SELECT 100
    UNION ALL SELECT 250
) t
JOIN token_stats ts ON holders.mint_address = ts.mint_address
WHERE holder_rank <= t.top_n
GROUP BY holders.mint_address, ts.timestamp, t.top_n
"#;

pub const TOKEN_DISTRIBUTION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_distribution_mv
TO token_distribution
AS WITH supply AS (
    SELECT mint_address, timestamp, any(supply) as supply
    FROM token_stats
    GROUP BY mint_address, timestamp
),
balances AS (
    SELECT 
        mint_address,
        sumMerge(total_amount) as balance
    FROM token_holder_balances_mv
    GROUP BY mint_address
),
metrics AS (
    SELECT
        t1.mint_address as mint_address,
        ts.timestamp as timestamp,
        t1.balance as balance,
        t2.balance as balance2,
        ts.supply as supply
    FROM balances t1
    CROSS JOIN balances t2
    JOIN supply ts ON t1.mint_address = ts.mint_address
)
SELECT
    mint_address,
    timestamp,
    sum(pow((balance / supply) * 100, 2)) as hhi,
    count() as denominator,
    CASE 
        WHEN count() > 0 THEN (1 - (
            sum(abs(balance - balance2)) / 
            (2 * count() * supply / count())
        )) * 100 
        ELSE 0 
    END as distribution_score
FROM metrics
GROUP BY mint_address, timestamp, supply
"#;