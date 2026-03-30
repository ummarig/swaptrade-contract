use soroban_sdk::{contracttype, Env, Symbol, symbol_short};

/// Network congestion level enum for categorizing congestion severity
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum CongestionLevel {
    /// Very low congestion (0-20% of capacity)
    VeryLow,
    /// Low congestion (20-40% of capacity)
    Low,
    /// Moderate congestion (40-60% of capacity)
    Moderate,
    /// High congestion (60-80% of capacity)
    High,
    /// Critical congestion (80-100%+ of capacity)
    Critical,
}

/// Network metrics snapshot at a point in time
#[derive(Clone, Debug)]
#[contracttype]
pub struct NetworkMetrics {
    /// Current transaction volume in the last minute (transactions per second)
    pub txn_volume_tps: u64,
    /// Average gas price in stroops (1 stroop = 0.00000001 XLM)
    pub avg_gas_price: u64,
    /// Estimated pending transactions in network queue
    pub pending_txn_count: u64,
    /// Current ledger close time (in seconds)
    pub ledger_close_time: u64,
    /// Estimated network capacity utilization (0-100, with 100 being full)
    pub capacity_utilization_percent: u32,
    /// Timestamp when metrics were sampled
    pub timestamp: u64,
    /// Average transaction confirmation time (in milliseconds)
    pub avg_confirmation_time_ms: u32,
}

/// Congestion trend indicating if congestion is increasing, decreasing, or stable
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum CongestionTrend {
    /// Congestion is increasing
    Increasing,
    /// Congestion is stable
    Stable,
    /// Congestion is decreasing
    Decreasing,
}

/// Historical congestion data point for trend analysis
#[derive(Clone, Debug)]
#[contracttype]
pub struct CongestionDataPoint {
    /// The congestion level at this time
    pub level: CongestionLevel,
    /// Capacity utilization percentage
    pub capacity_utilization: u32,
    /// Timestamp of this data point
    pub timestamp: u64,
}

/// Constants for network congestion monitoring
pub struct NetworkCongestionConstants;

impl NetworkCongestionConstants {
    /// Maximum transactions per second the network can handle
    pub const MAX_TXN_CAPACITY_TPS: u64 = 1000;
    
    /// Minimum gas price for transactions (in stroops)
    pub const MIN_GAS_PRICE: u64 = 100;
    
    /// Maximum gas price threshold (triggering critical congestion)
    pub const CRITICAL_GAS_PRICE: u64 = 5000;
    
    /// High congestion threshold (stroops)
    pub const HIGH_GAS_PRICE: u64 = 3000;
    
    /// Pending transaction queue threshold
    pub const HIGH_PENDING_TXN_THRESHOLD: u64 = 5000;
    
    /// Critical pending transaction threshold
    pub const CRITICAL_PENDING_TXN_THRESHOLD: u64 = 10000;
    
    /// Capacity utilization levels
    pub const VERY_LOW_CAPACITY_THRESHOLD: u32 = 20;
    pub const LOW_CAPACITY_THRESHOLD: u32 = 40;
    pub const MODERATE_CAPACITY_THRESHOLD: u32 = 60;
    pub const HIGH_CAPACITY_THRESHOLD: u32 = 80;
}

/// Network congestion monitoring module
pub struct NetworkCongestionMonitor;

impl NetworkCongestionMonitor {
    /// Storage key for current network metrics
    pub const METRICS_KEY: Symbol = symbol_short!("metrics");
    
    /// Storage key for historical data points
    pub const HISTORY_KEY: Symbol = symbol_short!("hist");
    
    /// Storage key for last recorded congestion level
    pub const LAST_LEVEL_KEY: Symbol = symbol_short!("lstlvl");

    /// Analyze current network metrics and return congestion level
    pub fn get_current_congestion_level(metrics: &NetworkMetrics) -> CongestionLevel {
        let mut score = 0u32;
        let mut weight_sum = 0u32;

        // Factor 1: Gas price (weight: 30%)
        let gas_factor = Self::calculate_gas_factor(metrics.avg_gas_price);
        score = score.saturating_add(gas_factor.saturating_mul(30));
        weight_sum = weight_sum.saturating_add(30);

        // Factor 2: Transaction volume (weight: 25%)
        let volume_factor = Self::calculate_volume_factor(metrics.txn_volume_tps);
        score = score.saturating_add(volume_factor.saturating_mul(25));
        weight_sum = weight_sum.saturating_add(25);

        // Factor 3: Pending transactions (weight: 25%)
        let pending_factor = Self::calculate_pending_factor(metrics.pending_txn_count);
        score = score.saturating_add(pending_factor.saturating_mul(25));
        weight_sum = weight_sum.saturating_add(25);

        // Factor 4: Capacity utilization (weight: 20%)
        let capacity_factor = metrics.capacity_utilization_percent.min(100) as u32;
        score = score.saturating_add(capacity_factor.saturating_mul(20));
        weight_sum = weight_sum.saturating_add(20);

        let weighted_score = if weight_sum > 0 {
            (score as u64) / (weight_sum as u64)
        } else {
            0
        };

        Self::score_to_congestion_level(weighted_score as u32)
    }

    /// Calculate gas price factor (0-100 scale)
    fn calculate_gas_factor(avg_gas_price: u64) -> u32 {
        match avg_gas_price {
            p if p <= NetworkCongestionConstants::MIN_GAS_PRICE => 0,
            p if p >= NetworkCongestionConstants::CRITICAL_GAS_PRICE => 100,
            p => {
                let range = NetworkCongestionConstants::CRITICAL_GAS_PRICE - NetworkCongestionConstants::MIN_GAS_PRICE;
                let diff = p - NetworkCongestionConstants::MIN_GAS_PRICE;
                ((diff as u128 * 100) / (range as u128)) as u32
            }
        }
    }

    /// Calculate transaction volume factor (0-100 scale)
    fn calculate_volume_factor(txn_volume_tps: u64) -> u32 {
        match txn_volume_tps {
            v if v == 0 => 0,
            v if v >= NetworkCongestionConstants::MAX_TXN_CAPACITY_TPS => 100,
            v => {
                ((v as u128 * 100) / (NetworkCongestionConstants::MAX_TXN_CAPACITY_TPS as u128)) as u32
            }
        }
    }

    /// Calculate pending transactions factor (0-100 scale)
    fn calculate_pending_factor(pending_txn_count: u64) -> u32 {
        match pending_txn_count {
            p if p == 0 => 0,
            p if p >= NetworkCongestionConstants::CRITICAL_PENDING_TXN_THRESHOLD => 100,
            p => {
                ((p as u128 * 100) / (NetworkCongestionConstants::CRITICAL_PENDING_TXN_THRESHOLD as u128)) as u32
            }
        }
    }

    /// Convert a 0-100 score to congestion level
    fn score_to_congestion_level(score: u32) -> CongestionLevel {
        match score {
            0..=20 => CongestionLevel::VeryLow,
            21..=40 => CongestionLevel::Low,
            41..=60 => CongestionLevel::Moderate,
            61..=80 => CongestionLevel::High,
            _ => CongestionLevel::Critical,
        }
    }

    /// Determine congestion trend based on current and previous metrics
    pub fn calculate_trend(current_metrics: &NetworkMetrics, previous_metrics: &NetworkMetrics) -> CongestionTrend {
        let current_level = Self::get_current_congestion_level(current_metrics);
        let previous_level = Self::get_current_congestion_level(previous_metrics);

        match (current_level, previous_level) {
            // Map increasing trend
            (CongestionLevel::Critical, CongestionLevel::High) |
            (CongestionLevel::Critical, CongestionLevel::Moderate) |
            (CongestionLevel::Critical, CongestionLevel::Low) |
            (CongestionLevel::Critical, CongestionLevel::VeryLow) |
            (CongestionLevel::High, CongestionLevel::Moderate) |
            (CongestionLevel::High, CongestionLevel::Low) |
            (CongestionLevel::High, CongestionLevel::VeryLow) |
            (CongestionLevel::Moderate, CongestionLevel::Low) |
            (CongestionLevel::Moderate, CongestionLevel::VeryLow) |
            (CongestionLevel::Low, CongestionLevel::VeryLow) => CongestionTrend::Increasing,

            // Map stable trend (same level or marginal changes)
            (a, b) if a == b => CongestionTrend::Stable,

            // Map decreasing trend
            (CongestionLevel::Low, CongestionLevel::High) |
            (CongestionLevel::Low, CongestionLevel::Critical) |
            (CongestionLevel::VeryLow, _) |
            (CongestionLevel::Low, CongestionLevel::Moderate) |
            (CongestionLevel::Moderate, CongestionLevel::High) |
            (CongestionLevel::Moderate, CongestionLevel::Critical) |
            (CongestionLevel::High, CongestionLevel::Critical) => CongestionTrend::Decreasing,
        }
    }

    /// Get numerical score for a congestion level (for fine-grained adjustments)
    pub fn get_level_score(level: CongestionLevel) -> u32 {
        match level {
            CongestionLevel::VeryLow => 10,
            CongestionLevel::Low => 30,
            CongestionLevel::Moderate => 50,
            CongestionLevel::High => 70,
            CongestionLevel::Critical => 90,
        }
    }

    /// Estimate if network is recovering (trend + time since change)
    pub fn is_recovering(trend: CongestionTrend, current_time: u64, trend_start_time: u64) -> bool {
        if trend != CongestionTrend::Decreasing {
            return false;
        }
        // Allow recovery if decreasing trend has been in effect for at least 30 seconds
        (current_time - trend_start_time) >= 30
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gas_price_factor_calculation() {
        // Min price should be 0
        assert_eq!(NetworkCongestionMonitor::calculate_gas_factor(100), 0);
        
        // Critical price should be 100
        assert_eq!(NetworkCongestionMonitor::calculate_gas_factor(5000), 100);
        
        // Mid-point should be around 50
        let mid = NetworkCongestionMonitor::calculate_gas_factor(2550);
        assert!(mid > 40 && mid < 60);
    }

    #[test]
    fn test_volume_factor_calculation() {
        assert_eq!(NetworkCongestionMonitor::calculate_volume_factor(0), 0);
        assert_eq!(NetworkCongestionMonitor::calculate_volume_factor(1000), 100);
        assert_eq!(NetworkCongestionMonitor::calculate_volume_factor(500), 50);
    }

    #[test]
    fn test_pending_factor_calculation() {
        assert_eq!(NetworkCongestionMonitor::calculate_pending_factor(0), 0);
        assert_eq!(NetworkCongestionMonitor::calculate_pending_factor(10000), 100);
        assert_eq!(NetworkCongestionMonitor::calculate_pending_factor(5000), 50);
    }

    #[test]
    fn test_score_to_congestion_level() {
        assert_eq!(NetworkCongestionMonitor::score_to_congestion_level(10), CongestionLevel::VeryLow);
        assert_eq!(NetworkCongestionMonitor::score_to_congestion_level(30), CongestionLevel::Low);
        assert_eq!(NetworkCongestionMonitor::score_to_congestion_level(50), CongestionLevel::Moderate);
        assert_eq!(NetworkCongestionMonitor::score_to_congestion_level(70), CongestionLevel::High);
        assert_eq!(NetworkCongestionMonitor::score_to_congestion_level(90), CongestionLevel::Critical);
    }

    #[test]
    fn test_get_current_congestion_level_very_low() {
        let metrics = NetworkMetrics {
            txn_volume_tps: 100,
            avg_gas_price: 100,
            pending_txn_count: 100,
            ledger_close_time: 1000,
            capacity_utilization_percent: 10,
            timestamp: 1000,
            avg_confirmation_time_ms: 2000,
        };
        assert_eq!(NetworkCongestionMonitor::get_current_congestion_level(&metrics), CongestionLevel::VeryLow);
    }

    #[test]
    fn test_get_current_congestion_level_critical() {
        let metrics = NetworkMetrics {
            txn_volume_tps: 1000,
            avg_gas_price: 5000,
            pending_txn_count: 10000,
            ledger_close_time: 1000,
            capacity_utilization_percent: 100,
            timestamp: 1000,
            avg_confirmation_time_ms: 10000,
        };
        assert_eq!(NetworkCongestionMonitor::get_current_congestion_level(&metrics), CongestionLevel::Critical);
    }

    #[test]
    fn test_calculate_trend_increasing() {
        let prev_metrics = NetworkMetrics {
            txn_volume_tps: 100,
            avg_gas_price: 100,
            pending_txn_count: 100,
            ledger_close_time: 1000,
            capacity_utilization_percent: 10,
            timestamp: 1000,
            avg_confirmation_time_ms: 2000,
        };

        let curr_metrics = NetworkMetrics {
            txn_volume_tps: 800,
            avg_gas_price: 3500,
            pending_txn_count: 8000,
            ledger_close_time: 1100,
            capacity_utilization_percent: 80,
            timestamp: 1100,
            avg_confirmation_time_ms: 8000,
        };

        let trend = NetworkCongestionMonitor::calculate_trend(&curr_metrics, &prev_metrics);
        assert_eq!(trend, CongestionTrend::Increasing);
    }
}
