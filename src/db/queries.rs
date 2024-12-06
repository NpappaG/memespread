use clickhouse::Client;
use anyhow::Result;

#[allow(dead_code)]
pub async fn get_tokens_needing_stats_update(client: &Client) -> Result<Vec<String>> {
    let query = "
        SELECT mint_address 
        FROM monitored_tokens 
        WHERE last_stats_update < now() - INTERVAL 1 MINUTE
    ";
    
    let mut cursor = client.query(query).fetch::<String>()?;
    let mut results = Vec::new();
    
    while let Some(row) = cursor.next().await? {
        results.push(row);
    }
    
    Ok(results)
}

#[allow(dead_code)]
pub async fn get_tokens_needing_metrics_update(client: &Client) -> Result<Vec<String>> {
    let query = "
        SELECT mint_address 
        FROM monitored_tokens 
        WHERE last_metrics_update < now() - INTERVAL 4 HOUR
    ";
    
    let mut cursor = client.query(query).fetch::<String>()?;
    let mut results = Vec::new();
    
    while let Some(row) = cursor.next().await? {
        results.push(row);
    }
    
    Ok(results)
}