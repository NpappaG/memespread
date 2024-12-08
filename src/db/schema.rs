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
    PRIMARY KEY (mint_address, timestamp)
) ENGINE = ReplacingMergeTree()
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

// First MV: Clean holder balances (excludes filtered accounts)
pub const TOKEN_HOLDER_BALANCES_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_holder_balances_mv
ENGINE = AggregatingMergeTree()
ORDER BY (mint_address, holder_address)
AS SELECT
    th.mint_address,
    th.holder_address,
    sumState(th.amount) as total_amount
FROM token_holders th
LEFT JOIN excluded_accounts ea ON ea.address = th.holder_address
WHERE ea.address IS NULL
GROUP BY th.mint_address, th.holder_address
"#;

// Second MV: Threshold analysis (uses price for USD values)
pub const TOKEN_HOLDER_THRESHOLDS_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_holder_thresholds_mv
ENGINE = AggregatingMergeTree()
ORDER BY (mint_address, timestamp, usd_threshold)
AS SELECT
    thb.mint_address,
    ts.timestamp,
    mv.threshold / ts.price as token_threshold,
    mv.threshold as usd_threshold,
    countState() as holder_count,
    countState() * 100.0 / any(total_holders.count) as percentage
FROM token_holder_balances_mv thb
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
JOIN (
    SELECT mint_address, countState() as count 
    FROM token_holder_balances_mv 
    GROUP BY mint_address
) total_holders ON thb.mint_address = total_holders.mint_address
WHERE sumMerge(thb.total_amount) * ts.price >= mv.threshold
GROUP BY thb.mint_address, ts.timestamp, mv.threshold
"#;

// Concentration metrics MV
pub const TOKEN_CONCENTRATION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_concentration_mv
ENGINE = AggregatingMergeTree()
ORDER BY (mint_address, timestamp, top_n)
AS SELECT
    holders.mint_address,
    ts.timestamp,
    t.top_n,
    sumState(holders.total_amount) / any(ts.supply) * 100 as percentage
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

// Distribution metrics MV
pub const TOKEN_DISTRIBUTION_MV_SQL: &str = r#"
CREATE MATERIALIZED VIEW IF NOT EXISTS token_distribution_mv
ENGINE = AggregatingMergeTree()
ORDER BY (mint_address, timestamp)
AS WITH supply AS (
    SELECT mint_address, timestamp, any(supply) as supply
    FROM token_stats
    GROUP BY mint_address, timestamp
)
SELECT
    t1.mint_address,
    ts.timestamp,
    sumState(pow(sumMerge(t1.total_amount) / ts.supply * 100, 2)) as hhi,
    countState() as denominator,
    CASE 
        WHEN countMerge(denominator) > 0 THEN (1 - (
            sumState(abs(sumMerge(t1.total_amount) - sumMerge(t2.total_amount))) / 
            (2 * countMerge(denominator) * ts.supply / countMerge(denominator))
        )) * 100 
        ELSE 0 
    END as distribution_score
FROM token_holder_balances_mv t1
CROSS JOIN token_holder_balances_mv t2
JOIN supply ts ON t1.mint_address = ts.mint_address
GROUP BY t1.mint_address, ts.timestamp
"#;
