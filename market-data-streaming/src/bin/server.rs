use market_data_streaming::*;
use tokio;
use tracing::{info, error};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Market Data Streaming Server");

    // Create WebSocket server
    let server = WebSocketServer::new();

    // Create order book manager
    let orderbook_manager = OrderBookManager::new(100, 8, 8); // 100 levels depth, 8 decimal precision

    // Create connection manager
    let connection_manager = ConnectionManager::new(10000); // Max 10k connections

    // Create rate limiter
    let rate_limiter = RateLimiter::new();

    // Create data source manager
    let data_source_config = DataSourceConfig::default();
    let data_source_manager = DataSourceManager::new(data_source_config);

    // Add data sources
    let binance_source = std::sync::Arc::new(BinanceDataSource::new(None, None));
    let coinbase_source = std::sync::Arc::new(CoinbaseDataSource::new());

    data_source_manager.add_source(binance_source).await?;
    data_source_manager.add_source(coinbase_source).await?;

    // Subscribe to symbols
    data_source_manager.subscribe_to_symbol("BTCUSDT").await?;
    data_source_manager.subscribe_to_symbol("ETHUSDT").await?;

    // Start data collection
    data_source_manager.start_data_collection().await?;

    // Start WebSocket server
    let server_handle = {
        let server = server.clone();
        tokio::spawn(async move {
            if let Err(e) = server.start("127.0.0.1:8080").await {
                error!("WebSocket server error: {}", e);
            }
        })
    };

    // Start market data broadcasting
    let broadcast_handle = {
        let server = server.clone();
        let data_source_manager = data_source_manager.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
            
            loop {
                interval.tick().await;
                
                // Get data from all sources
                for symbol in ["BTCUSDT", "ETHUSDT"] {
                    if let Ok(data_points) = data_source_manager.get_aggregated_data(symbol).await {
                        for data in data_points {
                            if let Err(e) = server.broadcast_market_data(&data).await {
                                error!("Failed to broadcast market data: {}", e);
                            }
                        }
                    }
                }
            }
        })
    };

    // Start periodic cleanup tasks
    let cleanup_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        
        loop {
            interval.tick().await;
            
            // Clean up idle connections
            if let Err(e) = connection_manager.cleanup_idle_connections(std::time::Duration::from_secs(300)).await {
                error!("Cleanup error: {}", e);
            }
            
            // Clean up expired penalties
            rate_limiter.cleanup_expired_penalties();
            
            // Health check data sources
            if let Err(e) = data_source_manager.health_check().await {
                error!("Health check error: {}", e);
            }
        }
    });

    info!("Market Data Streaming Server started on ws://127.0.0.1:8080");

    // Wait for tasks
    tokio::try_join!(server_handle, broadcast_handle, cleanup_handle)?;
    
    Ok(())
}
