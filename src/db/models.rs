use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct TokenStatsRecord {
    pub mint_address: String,
    pub timestamp: DateTime<Utc>,
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: u8,
    pub holders: u32,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct TokenHolderThresholdRecord {
    pub mint_address: String,
    pub timestamp: DateTime<Utc>,
    pub usd_threshold: f64,
    pub holder_count: u32,
    pub percentage: f64,
    pub percentage_of_10: f64,
}

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct TokenConcentrationMetricRecord {
    pub mint_address: String,
    pub timestamp: DateTime<Utc>,
    pub top_n: u32,
    pub percentage: f64,
}