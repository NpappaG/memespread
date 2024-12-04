//use sqlx::FromRow;
//use chrono::{DateTime, Utc};
use sqlx::types::Json;
use crate::types::{HolderThreshold, ConcentrationMetric};

#[derive(FromRow)]
pub struct TokenStatsRecord {
    pub id: i64,
    pub mint_address: String,
    pub timestamp: DateTime<Utc>,
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: i32,
    pub holders: i32,
    pub holder_thresholds: Json<Vec<HolderThreshold>>,
    pub concentration_metrics: Json<Vec<ConcentrationMetric>>,
}