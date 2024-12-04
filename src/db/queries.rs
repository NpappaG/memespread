//use sqlx::PgPool;
//use anyhow::Result;
//use crate::types::{TokenHolderStats, HistoricalStats};

//pub async fn insert_token_stats(
    //pool: &PgPool,
    //mint_address: &str,
    //stats: &TokenHolderStats,
//) -> Result<()> {
    //sqlx::query!(
        //r#"
        //INSERT INTO token_stats 
            //(mint_address, timestamp, price, supply, market_cap, decimals, 
             //holder_thresholds, concentration_metrics, hhi, distribution_score)
        //VALUES ($1, NOW(), $2, $3, $4, $5, $6, $7, $8, $9)
        //"#,
        //mint_address,
        //stats.price,
        //stats.supply,
        //stats.market_cap,
        //stats.decimals as i32,
        //sqlx::types::Json(&stats.holder_thresholds) as _,
        //sqlx::types::Json(&stats.concentration_metrics) as _,
        //stats.hhi,
        //stats.distribution_score,
    //)
    //.execute(pool)
    //.await?;

    //Ok(())
//}

//pub async fn get_token_history(
    //pool: &PgPool,
    //mint_address: &str,
    //days: i32,
//) -> Result<Vec<HistoricalStats>> {
    //let rows = sqlx::query!(
        //r#"
        //SELECT 
            //timestamp,
            //price,
            //supply,
            //market_cap,
            //decimals,
            //holder_thresholds as "holder_thresholds!: Json<Vec<_>>",
            //concentration_metrics as "concentration_metrics!: Json<Vec<_>>",
            //hhi,
            //distribution_score
        //FROM token_stats
        //WHERE mint_address = $1
          //AND timestamp > NOW() - ($2 || ' days')::INTERVAL
        //ORDER BY timestamp DESC
        //"#,
        //mint_address,
        //days
    //)
    //.fetch_all(pool)
    //.await?;

    //let history = rows.into_iter()
        //.map(|row| HistoricalStats {
            //timestamp: row.timestamp,
            //stats: TokenHolderStats {
                //price: row.price,
                //supply: row.supply,
                //market_cap: row.market_cap,
                //decimals: row.decimals as u8,
                //holder_thresholds: row.holder_thresholds.0,
                //concentration_metrics: row.concentration_metrics.0,
                //hhi: row.hhi,
                //distribution_score: row.distribution_score,
            //}
        //})
        //.collect();

    //Ok(history)
//}

//pub async fn is_token_monitored(
    //pool: &PgPool,
    //mint_address: &str,
//) -> Result<bool> {
    //let result = sqlx::query!(
        //r#"
        //SELECT EXISTS(
            //SELECT 1 FROM token_stats 
            //WHERE mint_address = $1
        //) as "exists!"
        //"#,
        //mint_address
    //)
    //.fetch_one(pool)
    //.await?;

    //Ok(result.exists)
//}