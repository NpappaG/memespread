use clickhouse::Row;
use time::OffsetDateTime;

#[allow(dead_code)]
#[derive(Debug, Row, serde::Deserialize)]
pub struct TokenStatsRecord {
    pub price: f64,
    pub supply: f64,
    pub market_cap: f64,
    pub decimals: u8,
}

#[allow(dead_code)]
#[derive(Debug, Row, serde::Deserialize)]
pub struct TokenHolderThresholdRecord {
    pub mint_address: String,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: OffsetDateTime,
    pub usd_threshold: f64,
    pub holder_count: u32,
    pub percentage: f64,
}

#[allow(dead_code)]
#[derive(Debug, Row, serde::Deserialize)]
pub struct TokenConcentrationMetricRecord {
    pub mint_address: String,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: OffsetDateTime,
    pub top_n: u32,
    pub percentage: f64,
}

#[allow(dead_code)]
#[derive(Debug, Row, serde::Deserialize)]
pub struct TokenDistributionMetricRecord {
    pub mint_address: String,
    #[serde(with = "clickhouse::serde::time::datetime")]
    pub timestamp: OffsetDateTime,
    pub hhi: f64,
    pub distribution_score: f64,
}
