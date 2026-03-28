use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketData {
    pub symbol: String,
    pub timestamp: DateTime<Utc>,
    pub data_type: MarketDataType,
    pub sequence: u64,
    pub checksum: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketDataType {
    Trade(TradeData),
    OrderBook(OrderBookData),
    Quote(QuoteData),
    Ticker(TickerData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeData {
    pub price: f64,
    pub quantity: f64,
    pub side: OrderSide,
    pub trade_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookData {
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub last_update_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceLevel {
    pub price: f64,
    pub quantity: f64,
    pub orders_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteData {
    pub bid_price: f64,
    pub bid_quantity: f64,
    pub ask_price: f64,
    pub ask_quantity: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerData {
    pub last_price: f64,
    pub volume_24h: f64,
    pub high_24h: f64,
    pub low_24h: f64,
    pub price_change_24h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSocketMessage {
    pub id: Uuid,
    pub message_type: MessageType,
    pub payload: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub compressed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageType {
    MarketData,
    Subscribe,
    Unsubscribe,
    Heartbeat,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionRequest {
    pub symbols: Vec<String>,
    pub data_types: Vec<MarketDataType>,
    pub compression: CompressionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionType {
    None,
    LZ4,
    Zstd,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionInfo {
    pub id: Uuid,
    pub remote_addr: String,
    pub connected_at: DateTime<Utc>,
    pub subscriptions: HashMap<String, Vec<MarketDataType>>,
    pub message_count: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalDataRequest {
    pub symbol: String,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub data_type: MarketDataType,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub latency_ms: f64,
    pub throughput_mbps: f64,
    pub connection_count: u32,
    pub message_rate: f64,
    pub compression_ratio: f64,
    pub error_rate: f64,
}
