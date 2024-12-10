use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenQuery {
    pub mint_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenHolderStats {
    pub mint_address: String,
    pub token_stats: TokenStats,
    pub distribution_stats: DistributionStats,
    pub holder_thresholds: Vec<HolderThreshold>,
    pub concentration_metrics: Vec<ConcentrationMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenStats {
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionStats {
    pub total_count: usize,
    pub hhi: f64,
    pub distribution_score: f64,
    pub median_balance: f64,
    pub mean_balance: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderThreshold {
    pub usd_threshold: f64,
    pub holder_count: u64,
    pub total_holders: u64,
    pub pct_total_holders: f64,
    pub pct_of_10usd: f64,
    pub mcap_per_holder: f64,
    pub slice_value_usd: f64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcentrationMetric {
    pub top_n: i32,
    pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedHolderThresholds {
    pub timestamp: DateTime<Utc>,
    pub thresholds: Vec<HolderThreshold>
}

//#[derive(Clone, Debug, Serialize, Deserialize)]
//pub struct HistoricalStats {
//    pub timestamp: DateTime<Utc>,
//    pub stats: TokenHolderStats,
//}
