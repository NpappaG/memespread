use serde::{Deserialize, Serialize};
//use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TokenQuery {
    pub mint_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenHolderStats {
    pub mint_address: String,
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: u8,
    pub total_count: usize,
    pub holder_thresholds: Vec<HolderThreshold>,
    pub concentration_metrics: Vec<ConcentrationMetric>,
    pub hhi: f64,
    pub distribution_score: f64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HolderThreshold {
    pub usd_threshold: f64,
    pub count: u64,
    pub percentage: f64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcentrationMetric {
    pub top_n: i32,
    pub percentage: f64,
}

//#[derive(Clone, Debug, Serialize, Deserialize)]
//pub struct HistoricalStats {
//    pub timestamp: DateTime<Utc>,
//    pub stats: TokenHolderStats,
//}
