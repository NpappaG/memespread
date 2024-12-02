//use std::sync::Arc;
//use solana_client::nonblocking::rpc_client::RpcClient;
//use governor::{RateLimiter, state::{NotKeyed, InMemoryState}, clock::DefaultClock};
//use crate::services::token::calculate_token_stats;

//pub async fn monitor_token(
    //rpc_client: Arc<RpcClient>,
    //rate_limiter: Arc<RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    //mint_address: String
//) {
    //loop {
        //match calculate_token_stats(&rpc_client, &rate_limiter, &mint_address).await {
            //Ok(stats) => {
                //tracing::info!("Token stats: {:?}", stats);
            //}
            //Err(e) => {
                //tracing::error!("Failed to get token stats: {}", e);
            //}
        //}
        //tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    //}
//}