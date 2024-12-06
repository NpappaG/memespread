ATTACH TABLE _ UUID 'd12fd9a5-d00c-4630-b94e-be56f549615b'
(
    `mint_address` String,
    `timestamp` DateTime DEFAULT now(),
    `hhi` Float64,
    `distribution_score` Float64
)
ENGINE = MergeTree
PRIMARY KEY (mint_address, timestamp)
ORDER BY (mint_address, timestamp)
SETTINGS index_granularity = 8192
