use clickhouse::Client;
use anyhow::Result;

pub async fn get_tokens_needing_stats_update(client: &Client) -> Result<Vec<String>> {
    let query = "
        SELECT mint_address 
        FROM monitored_tokens 
        WHERE last_stats_update < subtractMinutes(now(), 1)
    ";
    
    let mut cursor = client.query(query).fetch::<String>()?;
    let mut results = Vec::new();
    
    while let Some(row) = cursor.next().await? {
        results.push(row);
    }
    
    Ok(results)
}

pub async fn get_tokens_needing_metrics_update(client: &Client) -> Result<Vec<String>> {
    let query = "
        SELECT mint_address 
        FROM monitored_tokens 
        WHERE last_metrics_update < subtractHours(now(), 4)
    ";
    
    let mut cursor = client.query(query).fetch::<String>()?;
    let mut results = Vec::new();
    
    while let Some(row) = cursor.next().await? {
        results.push(row);
    }
    
    Ok(results)
}