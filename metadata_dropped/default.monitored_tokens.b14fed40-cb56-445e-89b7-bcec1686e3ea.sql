ATTACH TABLE _ UUID 'b14fed40-cb56-445e-89b7-bcec1686e3ea'
(
    `mint_address` String,
    `last_stats_update` DateTime,
    `last_metrics_update` DateTime,
    `created_at` DateTime DEFAULT now()
)
ENGINE = ReplacingMergeTree
PRIMARY KEY mint_address
ORDER BY mint_address
SETTINGS index_granularity = 8192
