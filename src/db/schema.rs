//// This will contain our SQL schema definitions
//pub const INIT_SQL: &str = r#"
//-- Enable TimescaleDB extension
//CREATE EXTENSION IF NOT EXISTS timescaledb;

//CREATE TABLE IF NOT EXISTS token_stats (
    //timestamp TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    //mint_address TEXT NOT NULL,
    //price DOUBLE PRECISION NOT NULL,
    //supply DOUBLE PRECISION NOT NULL,
    //market_cap DOUBLE PRECISION NOT NULL,
    //decimals INTEGER NOT NULL,
    //holder_thresholds JSONB NOT NULL,
    //concentration_metrics JSONB NOT NULL,
    //hhi DOUBLE PRECISION NOT NULL,
    //distribution_score DOUBLE PRECISION NOT NULL,
    
    //-- Add constraints for data quality
    //CONSTRAINT positive_price CHECK (price >= 0),
    //CONSTRAINT positive_supply CHECK (supply >= 0),
    //CONSTRAINT positive_market_cap CHECK (market_cap >= 0),
    //CONSTRAINT valid_distribution_score CHECK (distribution_score >= 0 AND distribution_score <= 100)
//);

//-- Convert to hypertable with compound partitioning
//SELECT create_hypertable('token_stats', 'timestamp',
    //partitioning_column => 'mint_address',
    //number_partitions => 4,
    //chunk_time_interval => INTERVAL '1 day',
    //create_default_indexes => TRUE);

//-- Create indexes for common query patterns
//CREATE INDEX IF NOT EXISTS idx_token_stats_mint_time 
    //ON token_stats(mint_address, timestamp DESC);

//-- Add compression policy (optional)
//-- SELECT add_compression_policy('token_stats', INTERVAL '7 days');

//COMMENT ON TABLE token_stats IS 'Time-series token statistics and metrics';
//"#;