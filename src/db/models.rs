use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::types::models::{HolderThreshold, ConcentrationMetric};

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenStatsRecord {
    pub mint_address: String,
    pub timestamp: DateTime<Utc>,
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: u8,
    pub holders: u32,
    pub holder_thresholds: Vec<HolderThreshold>,
    pub concentration_metrics: Vec<ConcentrationMetric>,
    pub hhi: f64,
    pub distribution_score: f64,
}