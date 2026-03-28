use crate::error::{MarketDataError, Result};
use crate::types::*;
use std::collections::BTreeMap;
use parking_lot::RwLock;
use std::sync::Arc;
use dashmap::DashMap;
use chrono::Utc;
use tracing::{debug, info};

pub struct OrderBookManager {
    orderbooks: Arc<DashMap<String, Arc<RwLock<OrderBook>>>>,
    aggregator: Arc<OrderBookAggregator>,
}

pub struct OrderBook {
    pub symbol: String,
    pub bids: BTreeMap<f64, PriceLevel>, // Sorted by price (descending for bids)
    pub asks: BTreeMap<f64, PriceLevel>, // Sorted by price (ascending for asks)
    pub last_update_id: u64,
    pub sequence: u64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

pub struct OrderBookAggregator {
    max_depth: usize,
    price_precision: u8,
    quantity_precision: u8,
}

impl OrderBookManager {
    pub fn new(max_depth: usize, price_precision: u8, quantity_precision: u8) -> Self {
        Self {
            orderbooks: Arc::new(DashMap::new()),
            aggregator: Arc::new(OrderBookAggregator::new(max_depth, price_precision, quantity_precision)),
        }
    }

    pub fn get_or_create_orderbook(&self, symbol: &str) -> Arc<RwLock<OrderBook>> {
        self.orderbooks
            .entry(symbol.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(OrderBook::new(symbol))))
            .clone()
    }

    pub fn update_orderbook(&self, symbol: &str, update: &OrderBookUpdate) -> Result<()> {
        let orderbook = self.get_or_create_orderbook(symbol);
        let mut ob = orderbook.write();
        
        ob.update(update, &self.aggregator)?;
        
        info!("Updated orderbook for {}: {} bids, {} asks", 
              symbol, ob.bids.len(), ob.asks.len());
        
        Ok(())
    }

    pub fn get_orderbook_snapshot(&self, symbol: &str, depth: Option<usize>) -> Result<OrderBookData> {
        let orderbook = self.orderbooks.get(symbol)
            .ok_or_else(|| MarketDataError::Validation(format!("Orderbook not found for symbol: {}", symbol)))?;
        
        let ob = orderbook.read();
        Ok(ob.get_snapshot(depth.unwrap_or(self.aggregator.max_depth)))
    }

    pub fn get_spread(&self, symbol: &str) -> Result<f64> {
        let orderbook = self.orderbooks.get(symbol)
            .ok_or_else(|| MarketDataError::Validation(format!("Orderbook not found for symbol: {}", symbol)))?;
        
        let ob = orderbook.read();
        ob.get_spread()
    }

    pub fn get_best_bid_ask(&self, symbol: &str) -> Result<(f64, f64, f64, f64)> {
        let orderbook = self.orderbooks.get(symbol)
            .ok_or_else(|| MarketDataError::Validation(format!("Orderbook not found for symbol: {}", symbol)))?;
        
        let ob = orderbook.read();
        ob.get_best_bid_ask()
    }

    pub fn get_market_depth(&self, symbol: &str, levels: usize) -> Result<MarketDepth> {
        let orderbook = self.orderbooks.get(symbol)
            .ok_or_else(|| MarketDataError::Validation(format!("Orderbook not found for symbol: {}", symbol)))?;
        
        let ob = orderbook.read();
        Ok(ob.get_market_depth(levels))
    }

    pub fn calculate_volume_at_price(&self, symbol: &str, price: f64) -> Result<f64> {
        let orderbook = self.orderbooks.get(symbol)
            .ok_or_else(|| MarketDataError::Validation(format!("Orderbook not found for symbol: {}", symbol)))?;
        
        let ob = orderbook.read();
        Ok(ob.calculate_volume_at_price(price))
    }

    pub fn get_orderbook_stats(&self, symbol: &str) -> Result<OrderBookStats> {
        let orderbook = self.orderbooks.get(symbol)
            .ok_or_else(|| MarketDataError::Validation(format!("Orderbook not found for symbol: {}", symbol)))?;
        
        let ob = orderbook.read();
        Ok(ob.get_stats())
    }
}

#[derive(Debug, Clone)]
pub struct OrderBookUpdate {
    pub symbol: String,
    pub last_update_id: u64,
    pub bids: Vec<PriceLevelUpdate>,
    pub asks: Vec<PriceLevelUpdate>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct PriceLevelUpdate {
    pub price: f64,
    pub quantity: f64,
    pub action: UpdateAction,
}

#[derive(Debug, Clone)]
pub enum UpdateAction {
    Add,
    Update,
    Remove,
}

#[derive(Debug, Clone)]
pub struct MarketDepth {
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub bid_volume: f64,
    pub ask_volume: f64,
    pub weighted_bid_price: f64,
    pub weighted_ask_price: f64,
}

#[derive(Debug, Clone)]
pub struct OrderBookStats {
    pub total_bid_volume: f64,
    pub total_ask_volume: f64,
    pub bid_levels: usize,
    pub ask_levels: usize,
    pub spread: f64,
    pub mid_price: f64,
    pub imbalance_ratio: f64,
}

impl OrderBook {
    pub fn new(symbol: &str) -> Self {
        Self {
            symbol: symbol.to_string(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_id: 0,
            sequence: 0,
            last_updated: Utc::now(),
        }
    }

    pub fn update(&mut self, update: &OrderBookUpdate, aggregator: &OrderBookAggregator) -> Result<()> {
        // Validate update sequence
        if update.last_update_id <= self.last_update_id {
            return Err(MarketDataError::Validation("Stale orderbook update".to_string()));
        }

        // Process bid updates
        for bid_update in &update.bids {
            self.update_price_level(&mut self.bids, bid_update, aggregator, true)?;
        }

        // Process ask updates
        for ask_update in &update.asks {
            self.update_price_level(&mut self.asks, ask_update, aggregator, false)?;
        }

        self.last_update_id = update.last_update_id;
        self.sequence += 1;
        self.last_updated = update.timestamp;

        // Maintain maximum depth
        self.maintain_depth(aggregator.max_depth);

        Ok(())
    }

    fn update_price_level(
        &mut self,
        side: &mut BTreeMap<f64, PriceLevel>,
        update: &PriceLevelUpdate,
        aggregator: &OrderBookAggregator,
        is_bid: bool,
    ) -> Result<()> {
        let price = aggregator.round_price(update.price);

        match update.action {
            UpdateAction::Add | UpdateAction::Update => {
                if update.quantity > 0.0 {
                    let quantity = aggregator.round_quantity(update.quantity);
                    side.insert(price, PriceLevel {
                        price,
                        quantity,
                        orders_count: 1, // Simplified - would track actual order count
                    });
                } else {
                    side.remove(&price);
                }
            }
            UpdateAction::Remove => {
                side.remove(&price);
            }
        }

        Ok(())
    }

    fn maintain_depth(&mut self, max_depth: usize) {
        // Remove excess levels from both sides
        while self.bids.len() > max_depth {
            if let Some(key) = self.bids.keys().next().cloned() {
                self.bids.remove(&key);
            }
        }

        while self.asks.len() > max_depth {
            if let Some(key) = self.asks.keys().next_back().cloned() {
                self.asks.remove(&key);
            }
        }
    }

    pub fn get_snapshot(&self, depth: usize) -> OrderBookData {
        let bids: Vec<PriceLevel> = self.bids
            .iter()
            .rev()
            .take(depth)
            .map(|(_, level)| level.clone())
            .collect();

        let asks: Vec<PriceLevel> = self.asks
            .iter()
            .take(depth)
            .map(|(_, level)| level.clone())
            .collect();

        OrderBookData {
            bids,
            asks,
            last_update_id: self.last_update_id,
        }
    }

    pub fn get_spread(&self) -> Result<f64> {
        let best_bid = self.bids.keys().rev().next()
            .ok_or_else(|| MarketDataError::Validation("No bids available".to_string()))?;
        let best_ask = self.asks.keys().next()
            .ok_or_else(|| MarketDataError::Validation("No asks available".to_string()))?;
        
        Ok(best_ask - best_bid)
    }

    pub fn get_best_bid_ask(&self) -> Result<(f64, f64, f64, f64)> {
        let best_bid_price = *self.bids.keys().rev().next()
            .ok_or_else(|| MarketDataError::Validation("No bids available".to_string()))?;
        let best_bid_quantity = self.bids.get(&best_bid_price).unwrap().quantity;
        
        let best_ask_price = *self.asks.keys().next()
            .ok_or_else(|| MarketDataError::Validation("No asks available".to_string()))?;
        let best_ask_quantity = self.asks.get(&best_ask_price).unwrap().quantity;

        Ok((best_bid_price, best_bid_quantity, best_ask_price, best_ask_quantity))
    }

    pub fn get_market_depth(&self, levels: usize) -> MarketDepth {
        let bids: Vec<PriceLevel> = self.bids
            .iter()
            .rev()
            .take(levels)
            .map(|(_, level)| level.clone())
            .collect();

        let asks: Vec<PriceLevel> = self.asks
            .iter()
            .take(levels)
            .map(|(_, level)| level.clone())
            .collect();

        let bid_volume: f64 = bids.iter().map(|level| level.quantity).sum();
        let ask_volume: f64 = asks.iter().map(|level| level.quantity).sum();

        let weighted_bid_price = if bid_volume > 0.0 {
            bids.iter().map(|level| level.price * level.quantity).sum::<f64>() / bid_volume
        } else {
            0.0
        };

        let weighted_ask_price = if ask_volume > 0.0 {
            asks.iter().map(|level| level.price * level.quantity).sum::<f64>() / ask_volume
        } else {
            0.0
        };

        MarketDepth {
            bids,
            asks,
            bid_volume,
            ask_volume,
            weighted_bid_price,
            weighted_ask_price,
        }
    }

    pub fn calculate_volume_at_price(&self, price: f64) -> f64 {
        // Calculate total volume available at or better than the given price
        let mut volume = 0.0;

        // For bids, include all levels at or above the price
        for (bid_price, level) in self.bids.iter().rev() {
            if *bid_price >= price {
                volume += level.quantity;
            } else {
                break;
            }
        }

        // For asks, include all levels at or below the price
        for (ask_price, level) in self.asks.iter() {
            if *ask_price <= price {
                volume += level.quantity;
            } else {
                break;
            }
        }

        volume
    }

    pub fn get_stats(&self) -> OrderBookStats {
        let total_bid_volume: f64 = self.bids.values().map(|level| level.quantity).sum();
        let total_ask_volume: f64 = self.asks.values().map(|level| level.quantity).sum();
        
        let spread = self.get_spread().unwrap_or(0.0);
        let mid_price = if let (Some(best_bid), Some(best_ask)) = (self.bids.keys().rev().next(), self.asks.keys().next()) {
            (best_bid + best_ask) / 2.0
        } else {
            0.0
        };

        let imbalance_ratio = if total_bid_volume + total_ask_volume > 0.0 {
            (total_bid_volume - total_ask_volume) / (total_bid_volume + total_ask_volume)
        } else {
            0.0
        };

        OrderBookStats {
            total_bid_volume,
            total_ask_volume,
            bid_levels: self.bids.len(),
            ask_levels: self.asks.len(),
            spread,
            mid_price,
            imbalance_ratio,
        }
    }
}

impl OrderBookAggregator {
    pub fn new(max_depth: usize, price_precision: u8, quantity_precision: u8) -> Self {
        Self {
            max_depth,
            price_precision,
            quantity_precision,
        }
    }

    fn round_price(&self, price: f64) -> f64 {
        let multiplier = 10_f64.powi(self.price_precision as i32);
        (price * multiplier).round() / multiplier
    }

    fn round_quantity(&self, quantity: f64) -> f64 {
        let multiplier = 10_f64.powi(self.quantity_precision as i32);
        (quantity * multiplier).round() / multiplier
    }
}
