use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};
use crate::types::models::{HolderThreshold, ConcentrationMetric};

#[derive(Debug, Serialize, Deserialize, clickhouse::Row)]
pub struct TokenStatsRecord {
    pub mint_address: String,
    pub timestamp: DateTime<Utc>,
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: u8,
    pub holders: u32,
    #[serde(deserialize_with = "deserialize_json_string")]
    pub holder_thresholds: Vec<HolderThreshold>,
    #[serde(deserialize_with = "deserialize_json_string")]
    pub concentration_metrics: Vec<ConcentrationMetric>,
}

fn deserialize_json_string<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let s: String = String::deserialize(deserializer)?;
    serde_json::from_str(&s).map_err(serde::de::Error::custom)
}