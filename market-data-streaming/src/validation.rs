use crate::error::{MarketDataError, Result};
use crate::types::*;
use std::collections::HashSet;
use chrono::{DateTime, Utc, Duration};
use crc::{crc32, Crc};
use tracing::{debug, warn, error};

pub struct DataValidator {
    allowed_symbols: HashSet<String>,
    max_message_size: usize,
    max_price: f64,
    max_quantity: f64,
    price_precision: u8,
    quantity_precision: u8,
    max_timestamp_drift: Duration,
    crc_engine: Crc<u32>,
    validation_stats: parking_lot::RwLock<ValidationStats>,
}

#[derive(Debug, Default)]
pub struct ValidationStats {
    pub total_validations: u64,
    pub successful_validations: u64,
    pub failed_validations: u64,
    pub validation_errors: Vec<String>,
    pub average_validation_time: f64,
}

impl DataValidator {
    pub fn new() -> Self {
        Self {
            allowed_symbols: HashSet::new(),
            max_message_size: 1024 * 1024, // 1MB
            max_price: 1_000_000.0,
            max_quantity: 1_000_000.0,
            price_precision: 8,
            quantity_precision: 8,
            max_timestamp_drift: Duration::seconds(60), // 1 minute
            crc_engine: crc32::IEEE,
            validation_stats: parking_lot::RwLock::new(ValidationStats::default()),
        }
    }

    pub fn with_config(config: ValidationConfig) -> Self {
        let mut validator = Self::new();
        validator.allowed_symbols = config.allowed_symbols;
        validator.max_message_size = config.max_message_size;
        validator.max_price = config.max_price;
        validator.max_quantity = config.max_quantity;
        validator.price_precision = config.price_precision;
        validator.quantity_precision = config.quantity_precision;
        validator.max_timestamp_drift = config.max_timestamp_drift;
        validator
    }

    pub fn add_allowed_symbol(&mut self, symbol: String) {
        self.allowed_symbols.insert(symbol);
    }

    pub fn validate_market_data(&self, market_data: &MarketData) -> bool {
        let start_time = std::time::Instant::now();
        let mut result = true;

        // Update validation count
        {
            let mut stats = self.validation_stats.write();
            stats.total_validations += 1;
        }

        // Validate symbol
        if !self.validate_symbol(&market_data.symbol) {
            warn!("Invalid symbol: {}", market_data.symbol);
            result = false;
        }

        // Validate timestamp
        if !self.validate_timestamp(&market_data.timestamp) {
            warn!("Invalid timestamp: {}", market_data.timestamp);
            result = false;
        }

        // Validate data type specific fields
        if !self.validate_data_type(&market_data.data_type) {
            warn!("Invalid data type for symbol: {}", market_data.symbol);
            result = false;
        }

        // Validate checksum
        if !self.validate_checksum(market_data) {
            warn!("Invalid checksum for symbol: {}", market_data.symbol);
            result = false;
        }

        // Update statistics
        let validation_time = start_time.elapsed();
        {
            let mut stats = self.validation_stats.write();
            if result {
                stats.successful_validations += 1;
            } else {
                stats.failed_validations += 1;
            }
            
            let total_time = stats.average_validation_time * (stats.total_validations - 1) as f64;
            stats.average_validation_time = (total_time + validation_time.as_nanos() as f64) / stats.total_validations as f64;
        }

        debug!("Market data validation for {}: {} (took {:?})", 
               market_data.symbol, result, validation_time);

        result
    }

    pub fn validate_subscription(&self, subscription: &SubscriptionRequest) -> bool {
        // Validate symbols
        for symbol in &subscription.symbols {
            if !self.allowed_symbols.contains(symbol) {
                warn!("Subscription request for unknown symbol: {}", symbol);
                return false;
            }
        }

        // Validate data types
        for data_type in &subscription.data_types {
            if !self.is_valid_data_type(data_type) {
                warn!("Invalid data type in subscription request");
                return false;
            }
        }

        // Validate subscription size
        if subscription.symbols.len() > 100 {
            warn!("Subscription request exceeds maximum symbols limit");
            return false;
        }

        true
    }

    pub fn validate_websocket_message(&self, message: &WebSocketMessage) -> bool {
        // Validate message size
        if message.payload.len() > self.max_message_size {
            warn!("WebSocket message exceeds maximum size: {}", message.payload.len());
            return false;
        }

        // Validate timestamp
        if !self.validate_timestamp(&message.timestamp) {
            warn!("WebSocket message has invalid timestamp");
            return false;
        }

        // Validate message type
        match message.message_type {
            MessageType::MarketData => {
                // Try to deserialize and validate market data
                if let Ok(market_data) = bincode::deserialize::<MarketData>(&message.payload) {
                    self.validate_market_data(&market_data)
                } else {
                    warn!("Failed to deserialize market data from WebSocket message");
                    false
                }
            }
            MessageType::Subscribe | MessageType::Unsubscribe => {
                // Try to deserialize subscription request
                bincode::deserialize::<SubscriptionRequest>(&message.payload).is_ok()
            }
            MessageType::Heartbeat | MessageType::Error => true, // Always valid
        }
    }

    fn validate_symbol(&self, symbol: &str) -> bool {
        if self.allowed_symbols.is_empty() {
            return true; // Allow all symbols if no restrictions
        }

        self.allowed_symbols.contains(symbol)
    }

    fn validate_timestamp(&self, timestamp: &DateTime<Utc>) -> bool {
        let now = Utc::now();
        let drift = (now - *timestamp).abs();

        if drift > self.max_timestamp_drift {
            warn!("Timestamp drift too large: {:?}", drift);
            return false;
        }

        true
    }

    fn validate_data_type(&self, data_type: &MarketDataType) -> bool {
        match data_type {
            MarketDataType::Trade(trade) => {
                self.validate_price(trade.price) && 
                self.validate_quantity(trade.quantity) &&
                !trade.trade_id.is_empty()
            }
            MarketDataType::OrderBook(orderbook) => {
                self.validate_price_levels(&orderbook.bids) &&
                self.validate_price_levels(&orderbook.asks)
            }
            MarketDataType::Quote(quote) => {
                self.validate_price(quote.bid_price) &&
                self.validate_price(quote.ask_price) &&
                self.validate_quantity(quote.bid_quantity) &&
                self.validate_quantity(quote.ask_quantity) &&
                quote.ask_price > quote.bid_price
            }
            MarketDataType::Ticker(ticker) => {
                self.validate_price(ticker.last_price) &&
                self.validate_price(ticker.high_24h) &&
                self.validate_price(ticker.low_24h) &&
                ticker.volume_24h >= 0.0 &&
                ticker.high_24h >= ticker.low_24h
            }
        }
    }

    fn validate_price(&self, price: f64) -> bool {
        if price <= 0.0 || price > self.max_price {
            return false;
        }

        if !price.is_finite() {
            return false;
        }

        // Check precision
        let decimal_places = self.count_decimal_places(price);
        decimal_places <= self.price_precision as usize
    }

    fn validate_quantity(&self, quantity: f64) -> bool {
        if quantity <= 0.0 || quantity > self.max_quantity {
            return false;
        }

        if !quantity.is_finite() {
            return false;
        }

        // Check precision
        let decimal_places = self.count_decimal_places(quantity);
        decimal_places <= self.quantity_precision as usize
    }

    fn validate_price_levels(&self, levels: &[PriceLevel]) -> bool {
        for level in levels {
            if !self.validate_price(level.price) || !self.validate_quantity(level.quantity) {
                return false;
            }
        }

        // Check for duplicate prices
        let mut prices = HashSet::new();
        for level in levels {
            if !prices.insert(level.price) {
                return false; // Duplicate price found
            }
        }

        true
    }

    fn validate_checksum(&self, market_data: &MarketData) -> bool {
        // Calculate checksum of the data (excluding the checksum field)
        let serialized = match bincode::serialize(&market_data.data_type) {
            Ok(data) => data,
            Err(_) => return false,
        };

        let calculated_checksum = self.crc_engine.checksum(&serialized);
        calculated_checksum == market_data.checksum
    }

    fn count_decimal_places(&self, num: f64) -> usize {
        let string = format!("{:.15}", num);
        if let Some(decimal_pos) = string.find('.') {
            let decimal_part = &string[decimal_pos + 1..];
            decimal_part.trim_end_matches('0').len()
        } else {
            0
        }
    }

    fn is_valid_data_type(&self, data_type: &MarketDataType) -> bool {
        match data_type {
            MarketDataType::Trade(_) => true,
            MarketDataType::OrderBook(_) => true,
            MarketDataType::Quote(_) => true,
            MarketDataType::Ticker(_) => true,
        }
    }

    pub fn validate_order_book_integrity(&self, orderbook: &OrderBookData) -> bool {
        // Check that bids are sorted in descending order
        for window in orderbook.bids.windows(2) {
            if window[0].price <= window[1].price {
                return false;
            }
        }

        // Check that asks are sorted in ascending order
        for window in orderbook.asks.windows(2) {
            if window[0].price >= window[1].price {
                return false;
            }
        }

        // Check that best bid is less than best ask
        if let (Some(best_bid), Some(best_ask)) = (orderbook.bids.first(), orderbook.asks.first()) {
            if best_bid.price >= best_ask.price {
                return false;
            }
        }

        true
    }

    pub fn validate_trade_sequence(&self, trades: &[TradeData]) -> bool {
        if trades.len() < 2 {
            return true;
        }

        // Check for duplicate trade IDs
        let mut trade_ids = HashSet::new();
        for trade in trades {
            if !trade_ids.insert(&trade.trade_id) {
                return false; // Duplicate trade ID found
            }
        }

        // Validate each trade
        for trade in trades {
            if !self.validate_price(trade.price) || !self.validate_quantity(trade.quantity) {
                return false;
            }
        }

        true
    }

    pub fn get_validation_stats(&self) -> ValidationStats {
        self.validation_stats.read().clone()
    }

    pub fn reset_stats(&self) {
        let mut stats = self.validation_stats.write();
        *stats = ValidationStats::default();
    }
}

#[derive(Debug, Clone)]
pub struct ValidationConfig {
    pub allowed_symbols: HashSet<String>,
    pub max_message_size: usize,
    pub max_price: f64,
    pub max_quantity: f64,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub max_timestamp_drift: Duration,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            allowed_symbols: HashSet::new(),
            max_message_size: 1024 * 1024, // 1MB
            max_price: 1_000_000.0,
            max_quantity: 1_000_000.0,
            price_precision: 8,
            quantity_precision: 8,
            max_timestamp_drift: Duration::seconds(60),
        }
    }
}

pub struct DataIntegrityChecker {
    checksum_cache: moka::future::Cache<String, u32>,
    max_cache_size: u64,
}

impl DataIntegrityChecker {
    pub fn new(max_cache_size: u64) -> Self {
        Self {
            checksum_cache: moka::future::Cache::builder()
                .max_capacity(max_cache_size)
                .build(),
            max_cache_size,
        }
    }

    pub async fn verify_data_integrity(&self, data: &[u8], expected_checksum: u32) -> bool {
        let checksum = crc32::checksum_ieee(data);
        checksum == expected_checksum
    }

    pub async fn cache_checksum(&self, key: String, checksum: u32) {
        self.checksum_cache.insert(key, checksum).await;
    }

    pub async fn get_cached_checksum(&self, key: &str) -> Option<u32> {
        self.checksum_cache.get(key).await
    }

    pub async fn verify_sequence_integrity(&self, data_sequence: &[MarketData]) -> bool {
        if data_sequence.is_empty() {
            return true;
        }

        // Check for gaps in sequence numbers
        for window in data_sequence.windows(2) {
            if window[1].sequence != window[0].sequence + 1 {
                return false;
            }
        }

        // Verify each data item's checksum
        for data in data_sequence {
            let serialized = match bincode::serialize(&data.data_type) {
                Ok(s) => s,
                Err(_) => return false,
            };
            
            let calculated_checksum = crc32::checksum_ieee(&serialized);
            if calculated_checksum != data.checksum {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MarketDataType, TradeData, OrderSide};

    #[test]
    fn test_price_validation() {
        let validator = DataValidator::new();
        
        assert!(validator.validate_price(100.0));
        assert!(!validator.validate_price(-1.0));
        assert!(!validator.validate_price(f64::INFINITY));
        assert!(!validator.validate_price(2_000_000.0)); // Above max
    }

    #[test]
    fn test_quantity_validation() {
        let validator = DataValidator::new();
        
        assert!(validator.validate_quantity(1.0));
        assert!(!validator.validate_quantity(0.0));
        assert!(!validator.validate_quantity(-1.0));
        assert!(!validator.validate_quantity(f64::NAN));
    }

    #[test]
    fn test_market_data_validation() {
        let mut validator = DataValidator::new();
        validator.add_allowed_symbol("BTCUSDT".to_string());

        let market_data = MarketData {
            symbol: "BTCUSDT".to_string(),
            timestamp: Utc::now(),
            data_type: MarketDataType::Trade(TradeData {
                price: 50000.0,
                quantity: 0.1,
                side: OrderSide::Buy,
                trade_id: "12345".to_string(),
            }),
            sequence: 1,
            checksum: 0,
        };

        assert!(validator.validate_market_data(&market_data));
    }

    #[test]
    fn test_order_book_integrity() {
        let validator = DataValidator::new();

        let valid_orderbook = OrderBookData {
            bids: vec![
                PriceLevel { price: 50000.0, quantity: 1.0, orders_count: 1 },
                PriceLevel { price: 49999.0, quantity: 2.0, orders_count: 1 },
            ],
            asks: vec![
                PriceLevel { price: 50001.0, quantity: 1.0, orders_count: 1 },
                PriceLevel { price: 50002.0, quantity: 2.0, orders_count: 1 },
            ],
            last_update_id: 1,
        };

        assert!(validator.validate_order_book_integrity(&valid_orderbook));
    }
}
