use crate::error::{MarketDataError, Result};
use crate::types::*;
use std::sync::Arc;
use parking_lot::RwLock;
use dashmap::DashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tokio::sync::mpsc;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use tracing::{info, warn, error, debug};

pub struct DataSourceManager {
    sources: Arc<DashMap<String, Arc<dyn DataSource>>>,
    aggregators: Arc<DashMap<String, Arc<DataAggregator>>>,
    config: DataSourceConfig,
    stats: Arc<RwLock<DataSourceStats>>,
}

#[async_trait]
pub trait DataSource: Send + Sync {
    async fn connect(&mut self) -> Result<()>;
    async fn disconnect(&mut self) -> Result<()>;
    async fn subscribe(&mut self, symbols: &[String]) -> Result<()>;
    async fn unsubscribe(&mut self, symbols: &[String]) -> Result<()>;
    async fn get_market_data(&self) -> Result<MarketData>;
    fn is_connected(&self) -> bool;
    fn get_source_info(&self) -> DataSourceInfo;
    fn get_health_status(&self) -> HealthStatus;
}

pub struct DataAggregator {
    source_id: String,
    aggregation_rules: Vec<AggregationRule>,
    buffer: Arc<RwLock<Vec<MarketData>>>,
    output_tx: mpsc::UnboundedSender<MarketData>,
    stats: AggregatorStats,
}

#[derive(Debug, Clone)]
pub struct AggregationRule {
    pub rule_type: AggregationType,
    pub symbol: String,
    pub interval: std::time::Duration,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub enum AggregationType {
    Tick, // No aggregation, pass through
    OHLCV(std::time::Duration), // OHLCV candles
    VWAP(std::time::Duration), // Volume Weighted Average Price
    TWAP(std::time::Duration), // Time Weighted Average Price
    VolumeProfile(std::time::Duration), // Volume profile aggregation
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceInfo {
    pub id: String,
    pub name: String,
    pub source_type: SourceType,
    pub endpoint: String,
    pub supported_symbols: Vec<String>,
    pub latency_ms: f64,
    pub reliability_score: f64,
    pub last_update: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceType {
    Exchange,
    MarketDataVendor,
    Internal,
    Test,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct DataSourceConfig {
    pub max_sources: usize,
    pub connection_timeout_ms: u64,
    pub heartbeat_interval_ms: u64,
    pub retry_attempts: u32,
    pub retry_delay_ms: u64,
    pub enable_fallback: bool,
    pub priority_weights: HashMap<String, f32>,
}

#[derive(Debug, Default, Clone)]
pub struct DataSourceStats {
    pub total_sources: usize,
    pub connected_sources: usize,
    pub total_messages_received: u64,
    pub total_messages_sent: u64,
    pub average_latency: f64,
    pub error_count: u64,
    pub last_error: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AggregatorStats {
    pub messages_processed: u64,
    pub messages_output: u64,
    pub average_processing_time: f64,
    pub buffer_size: usize,
    pub last_output: Option<DateTime<Utc>>,
}

// Binance Data Source Implementation
pub struct BinanceDataSource {
    info: DataSourceInfo,
    connected: bool,
    subscriptions: Vec<String>,
    client: Option<reqwest::Client>,
}

impl BinanceDataSource {
    pub fn new(api_key: Option<String>, api_secret: Option<String>) -> Self {
        Self {
            info: DataSourceInfo {
                id: "binance".to_string(),
                name: "Binance Exchange".to_string(),
                source_type: SourceType::Exchange,
                endpoint: "https://api.binance.com".to_string(),
                supported_symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()], // Simplified
                latency_ms: 50.0,
                reliability_score: 0.95,
                last_update: Utc::now(),
            },
            connected: false,
            subscriptions: Vec::new(),
            client: Some(reqwest::Client::new()),
        }
    }
}

#[async_trait]
impl DataSource for BinanceDataSource {
    async fn connect(&mut self) -> Result<()> {
        info!("Connecting to Binance data source");
        
        // Simulate connection
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        self.connected = true;
        self.info.last_update = Utc::now();
        
        info!("Successfully connected to Binance data source");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("Disconnecting from Binance data source");
        self.connected = false;
        self.subscriptions.clear();
        Ok(())
    }

    async fn subscribe(&mut self, symbols: &[String]) -> Result<()> {
        if !self.connected {
            return Err(MarketDataError::DataSourceError("Not connected".to_string()));
        }

        for symbol in symbols {
            if !self.subscriptions.contains(symbol) {
                self.subscriptions.push(symbol.clone());
                info!("Subscribed to {} on Binance", symbol);
            }
        }
        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: &[String]) -> Result<()> {
        for symbol in symbols {
            self.subscriptions.retain(|s| s != symbol);
            info!("Unsubscribed from {} on Binance", symbol);
        }
        Ok(())
    }

    async fn get_market_data(&self) -> Result<MarketData> {
        if !self.connected {
            return Err(MarketDataError::DataSourceError("Not connected".to_string()));
        }

        // Simulate market data
        let symbol = self.subscriptions.first().unwrap_or(&"BTCUSDT".to_string()).clone();
        
        Ok(MarketData {
            symbol,
            timestamp: Utc::now(),
            data_type: MarketDataType::Trade(TradeData {
                price: 50000.0 + (rand::random::<f64>() - 0.5) * 1000.0,
                quantity: 0.1 + rand::random::<f64>() * 0.5,
                side: if rand::random() { OrderSide::Buy } else { OrderSide::Sell },
                trade_id: Uuid::new_v4().to_string(),
            }),
            sequence: rand::random(),
            checksum: rand::random(),
        })
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn get_source_info(&self) -> DataSourceInfo {
        self.info.clone()
    }

    fn get_health_status(&self) -> HealthStatus {
        if self.connected {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

// Coinbase Data Source Implementation
pub struct CoinbaseDataSource {
    info: DataSourceInfo,
    connected: bool,
    subscriptions: Vec<String>,
    client: Option<reqwest::Client>,
}

impl CoinbaseDataSource {
    pub fn new() -> Self {
        Self {
            info: DataSourceInfo {
                id: "coinbase".to_string(),
                name: "Coinbase Exchange".to_string(),
                source_type: SourceType::Exchange,
                endpoint: "https://api.pro.coinbase.com".to_string(),
                supported_symbols: vec!["BTC-USD".to_string(), "ETH-USD".to_string()], // Simplified
                latency_ms: 75.0,
                reliability_score: 0.92,
                last_update: Utc::now(),
            },
            connected: false,
            subscriptions: Vec::new(),
            client: Some(reqwest::Client::new()),
        }
    }
}

#[async_trait]
impl DataSource for CoinbaseDataSource {
    async fn connect(&mut self) -> Result<()> {
        info!("Connecting to Coinbase data source");
        
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        
        self.connected = true;
        self.info.last_update = Utc::now();
        
        info!("Successfully connected to Coinbase data source");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        info!("Disconnecting from Coinbase data source");
        self.connected = false;
        self.subscriptions.clear();
        Ok(())
    }

    async fn subscribe(&mut self, symbols: &[String]) -> Result<()> {
        if !self.connected {
            return Err(MarketDataError::DataSourceError("Not connected".to_string()));
        }

        for symbol in symbols {
            if !self.subscriptions.contains(symbol) {
                self.subscriptions.push(symbol.clone());
                info!("Subscribed to {} on Coinbase", symbol);
            }
        }
        Ok(())
    }

    async fn unsubscribe(&mut self, symbols: &[String]) -> Result<()> {
        for symbol in symbols {
            self.subscriptions.retain(|s| s != symbol);
            info!("Unsubscribed from {} on Coinbase", symbol);
        }
        Ok(())
    }

    async fn get_market_data(&self) -> Result<MarketData> {
        if !self.connected {
            return Err(MarketDataError::DataSourceError("Not connected".to_string()));
        }

        let symbol = self.subscriptions.first().unwrap_or(&"BTC-USD".to_string()).clone();
        
        Ok(MarketData {
            symbol,
            timestamp: Utc::now(),
            data_type: MarketDataType::Trade(TradeData {
                price: 50000.0 + (rand::random::<f64>() - 0.5) * 1000.0,
                quantity: 0.1 + rand::random::<f64>() * 0.5,
                side: if rand::random() { OrderSide::Buy } else { OrderSide::Sell },
                trade_id: Uuid::new_v4().to_string(),
            }),
            sequence: rand::random(),
            checksum: rand::random(),
        })
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn get_source_info(&self) -> DataSourceInfo {
        self.info.clone()
    }

    fn get_health_status(&self) -> HealthStatus {
        if self.connected {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

impl DataSourceManager {
    pub fn new(config: DataSourceConfig) -> Self {
        Self {
            sources: Arc::new(DashMap::new()),
            aggregators: Arc::new(DashMap::new()),
            config,
            stats: Arc::new(RwLock::new(DataSourceStats::default())),
        }
    }

    pub async fn add_source(&self, mut source: Arc<dyn DataSource>) -> Result<()> {
        let source_info = source.get_source_info();
        let source_id = source_info.id.clone();

        if self.sources.len() >= self.config.max_sources {
            return Err(MarketDataError::Validation("Maximum sources reached".to_string()));
        }

        // Connect to the source
        source.connect().await?;
        self.sources.insert(source_id.clone(), source);

        // Update stats
        {
            let mut stats = self.stats.write();
            stats.total_sources += 1;
            stats.connected_sources += 1;
        }

        info!("Added data source: {}", source_id);
        Ok(())
    }

    pub async fn remove_source(&self, source_id: &str) -> Result<()> {
        if let Some((_, source)) = self.sources.remove(source_id) {
            let mut src = source.as_ref().clone();
            src.disconnect().await?;

            // Update stats
            {
                let mut stats = self.stats.write();
                stats.total_sources -= 1;
                stats.connected_sources -= 1;
            }

            info!("Removed data source: {}", source_id);
            Ok(())
        } else {
            Err(MarketDataError::DataSourceError(format!("Source not found: {}", source_id)))
        }
    }

    pub async fn subscribe_to_symbol(&self, symbol: &str) -> Result<()> {
        for entry in self.sources.iter() {
            let source = entry.value();
            let mut src = source.as_ref().clone();
            if let Err(e) = src.subscribe(&[symbol.to_string()]).await {
                warn!("Failed to subscribe to {} on source {}: {}", 
                      symbol, source.get_source_info().id, e);
            }
        }
        Ok(())
    }

    pub async fn unsubscribe_from_symbol(&self, symbol: &str) -> Result<()> {
        for entry in self.sources.iter() {
            let source = entry.value();
            let mut src = source.as_ref().clone();
            if let Err(e) = src.unsubscribe(&[symbol.to_string()]).await {
                warn!("Failed to unsubscribe from {} on source {}: {}", 
                      symbol, source.get_source_info().id, e);
            }
        }
        Ok(())
    }

    pub async fn get_aggregated_data(&self, symbol: &str) -> Result<Vec<MarketData>> {
        let mut aggregated_data = Vec::new();

        for entry in self.sources.iter() {
            let source = entry.value();
            match source.get_market_data().await {
                Ok(data) if data.symbol == symbol => aggregated_data.push(data),
                Ok(_) => {} // Ignore data for other symbols
                Err(e) => {
                    warn!("Failed to get data from source {}: {}", 
                          source.get_source_info().id, e);
                }
            }
        }

        // Apply aggregation if configured
        if let Some(aggregator) = self.aggregators.get(symbol) {
            aggregated_data = aggregator.process_data(aggregated_data).await?;
        }

        Ok(aggregated_data)
    }

    pub async fn start_data_collection(&self) -> Result<()> {
        let sources = self.sources.clone();
        let stats = self.stats.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
            
            loop {
                interval.tick().await;

                for entry in sources.iter() {
                    let source = entry.value();
                    match source.get_market_data().await {
                        Ok(_) => {
                            // Update stats
                            stats.write().total_messages_received += 1;
                        }
                        Err(e) => {
                            stats.write().error_count += 1;
                            debug!("Error getting market data: {}", e);
                        }
                    }
                }
            }
        });

        info!("Started data collection from all sources");
        Ok(())
    }

    pub async fn health_check(&self) -> Result<HashMap<String, HealthStatus>> {
        let mut health_status = HashMap::new();

        for entry in self.sources.iter() {
            let source = entry.value();
            let status = source.get_health_status();
            health_status.insert(source.get_source_info().id.clone(), status);
        }

        Ok(health_status)
    }

    pub async def reconnect_failed_sources(&self) -> Result<usize> {
        let mut reconnected_count = 0;

        for entry in self.sources.iter() {
            let source = entry.value();
            if !source.is_connected() {
                let mut src = source.as_ref().clone();
                if let Ok(_) = src.connect().await {
                    reconnected_count += 1;
                    info!("Reconnected to source: {}", source.get_source_info().id);
                }
            }
        }

        Ok(reconnected_count)
    }

    pub fn get_source_info(&self, source_id: &str) -> Option<DataSourceInfo> {
        self.sources.get(source_id)
            .map(|source| source.get_source_info())
    }

    pub fn get_all_sources(&self) -> Vec<DataSourceInfo> {
        self.sources.iter()
            .map(|entry| entry.value().get_source_info())
            .collect()
    }

    pub fn get_statistics(&self) -> DataSourceStats {
        self.stats.read().clone()
    }
}

impl DataAggregator {
    pub fn new(source_id: String, output_tx: mpsc::UnboundedSender<MarketData>) -> Self {
        Self {
            source_id,
            aggregation_rules: Vec::new(),
            buffer: Arc::new(RwLock::new(Vec::new())),
            output_tx,
            stats: AggregatorStats::default(),
        }
    }

    pub fn add_rule(&mut self, rule: AggregationRule) {
        self.aggregation_rules.push(rule);
    }

    pub async fn process_data(&self, input_data: Vec<MarketData>) -> Result<Vec<MarketData>> {
        let start_time = std::time::Instant::now();
        
        // Store in buffer
        {
            let mut buffer = self.buffer.write();
            buffer.extend(input_data);
            
            // Keep buffer size manageable
            if buffer.len() > 10000 {
                buffer.drain(0..5000); // Remove oldest 5000 items
            }
        }

        // Apply aggregation rules
        let mut output_data = Vec::new();
        let buffer = self.buffer.read();

        for rule in &self.aggregation_rules {
            if !rule.enabled {
                continue;
            }

            match rule.rule_type {
                AggregationType::Tick => {
                    // Pass through data for this symbol
                    output_data.extend(buffer.iter()
                        .filter(|data| data.symbol == rule.symbol)
                        .cloned());
                }
                AggregationType::OHLCV(interval) => {
                    // OHLCV aggregation would be implemented here
                    // For now, just pass through
                    output_data.extend(buffer.iter()
                        .filter(|data| data.symbol == rule.symbol)
                        .cloned());
                }
                AggregationType::VWAP(_) => {
                    // VWAP aggregation would be implemented here
                    output_data.extend(buffer.iter()
                        .filter(|data| data.symbol == rule.symbol)
                        .cloned());
                }
                AggregationType::TWAP(_) => {
                    // TWAP aggregation would be implemented here
                    output_data.extend(buffer.iter()
                        .filter(|data| data.symbol == rule.symbol)
                        .cloned());
                }
                AggregationType::VolumeProfile(_) => {
                    // Volume profile aggregation would be implemented here
                    output_data.extend(buffer.iter()
                        .filter(|data| data.symbol == rule.symbol)
                        .cloned());
                }
            }
        }

        // Send output
        for data in &output_data {
            if let Err(_) = self.output_tx.send(data.clone()) {
                warn!("Failed to send aggregated data");
            }
        }

        // Update stats
        let processing_time = start_time.elapsed();
        debug!("Aggregated {} data points in {:?}", output_data.len(), processing_time);

        Ok(output_data)
    }
}

impl Default for DataSourceConfig {
    fn default() -> Self {
        Self {
            max_sources: 10,
            connection_timeout_ms: 5000,
            heartbeat_interval_ms: 30000,
            retry_attempts: 3,
            retry_delay_ms: 1000,
            enable_fallback: true,
            priority_weights: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_data_source_manager() {
        let config = DataSourceConfig::default();
        let manager = DataSourceManager::new(config);

        let binance_source = Arc::new(BinanceDataSource::new(None, None));
        assert!(manager.add_source(binance_source).await.is_ok());

        let sources = manager.get_all_sources();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, "binance");
    }

    #[tokio::test]
    async fn test_data_aggregator() {
        let (tx, _) = mpsc::unbounded_channel();
        let aggregator = DataAggregator::new("test".to_string(), tx);
        
        let rule = AggregationRule {
            rule_type: AggregationType::Tick,
            symbol: "BTCUSDT".to_string(),
            interval: std::time::Duration::from_secs(1),
            enabled: true,
        };
        
        let mut agg = aggregator;
        agg.add_rule(rule);
        
        let test_data = vec![];
        let result = agg.process_data(test_data).await;
        assert!(result.is_ok());
    }
}
