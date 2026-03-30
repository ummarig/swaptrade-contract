use soroban_sdk::{contracttype, Symbol, symbol_short};
use crate::network_congestion::{CongestionLevel, CongestionTrend, NetworkMetrics, NetworkCongestionMonitor};

/// Dynamic fee adjustment configuration parameters
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeAdjustmentConfig {
    /// Base fee in basis points (without congestion adjustment)
    pub base_fee_bps: u32,
    
    /// Multiplier for very low congestion (e.g., 0.8 = 80% of base fee)
    /// Stored as fixed-point: 100 = 1.0
    pub very_low_multiplier: u32,
    
    /// Multiplier for low congestion
    pub low_multiplier: u32,
    
    /// Multiplier for moderate congestion
    pub moderate_multiplier: u32,
    
    /// Multiplier for high congestion
    pub high_multiplier: u32,
    
    /// Multiplier for critical congestion
    pub critical_multiplier: u32,
    
    /// Maximum fee ceiling in basis points (prevents runaway fees)
    pub max_fee_bps: u32,
    
    /// Minimum fee floor in basis points (prevents fees going to zero)
    pub min_fee_bps: u32,
    
    /// Enable predictive surge pricing based on trend
    pub enable_trend_adjustment: bool,
    
    /// Trend multiplier adjustment (percentage, 0-50%)
    pub trend_adjustment_percent: u32,
    
    /// Minimum time between consecutive fee updates (in seconds)
    pub update_cooldown_seconds: u64,
    
    /// Enable emergency override for extreme congestion
    pub enable_emergency_override: bool,
    
    /// Emergency fee cap (basis points) when override is active
    pub emergency_fee_cap_bps: u32,
}

impl Default for FeeAdjustmentConfig {
    fn default() -> Self {
        Self {
            base_fee_bps: 50,                    // 0.5%
            very_low_multiplier: 80,             // 0.8x base fee
            low_multiplier: 90,                  // 0.9x base fee
            moderate_multiplier: 100,            // 1.0x base fee (no adjustment)
            high_multiplier: 150,                // 1.5x base fee
            critical_multiplier: 250,            // 2.5x base fee
            max_fee_bps: 500,                    // 5.0% hard cap
            min_fee_bps: 10,                     // 0.1% hard floor
            enable_trend_adjustment: true,
            trend_adjustment_percent: 20,        // 20% trend adjustment
            update_cooldown_seconds: 60,         // 1 minute between updates
            enable_emergency_override: true,
            emergency_fee_cap_bps: 300,          // 3.0% emergency cap
        }
    }
}

/// Fee adjustment calculation result
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeAdjustmentResult {
    /// Final calculated fee in basis points
    pub adjusted_fee_bps: u32,
    
    /// Base fee before adjustment
    pub base_fee_bps: u32,
    
    /// Multiplier applied due to congestion
    pub congestion_multiplier: u32,
    
    /// Additional adjustment from trend (in basis points)
    pub trend_adjustment_bps: i32,
    
    /// Current congestion level
    pub congestion_level: CongestionLevel,
    
    /// Trend direction
    pub trend: CongestionTrend,
    
    /// Whether emergency override was applied
    pub emergency_override_active: bool,
    
    /// Timestamp of this calculation
    pub calculated_at: u64,
}

/// Volatility statistics for predictive pricing
#[derive(Clone, Debug)]
#[contracttype]
pub struct CongestionVolatility {
    /// Average congestion score from recent samples
    pub avg_score: u32,
    
    /// Maximum congestion score seen recently
    pub max_score: u32,
    
    /// Minimum congestion score seen recently
    pub min_score: u32,
    
    /// Standard deviation of recent congestion scores
    pub volatility_score: u32,
}

/// Dynamic fee adjustment calculator
pub struct DynamicFeeAdjustment;

impl DynamicFeeAdjustment {
    /// Storage key for current fee configuration
    pub const CONFIG_KEY: Symbol = symbol_short!("fdynsrc");
    
    /// Storage key for current adjusted fees
    pub const CURRENT_FEES_KEY: Symbol = symbol_short!("curfees");
    
    /// Storage key for last fee update timestamp
    pub const LAST_UPDATE_KEY: Symbol = symbol_short!("lstupdt");
    
    /// Storage key for emergency override flag
    pub const EMERGENCY_OVERRIDE_KEY: Symbol = symbol_short!("emerovr");
    
    /// Fixed-point precision for multipliers (100 = 1.0)
    const MULTIPLIER_PRECISION: u32 = 100;

    /// Calculate adjusted fee based on current network metrics
    pub fn calculate_adjusted_fee(
        config: &FeeAdjustmentConfig,
        metrics: &NetworkMetrics,
        previous_metrics: Option<&NetworkMetrics>,
        current_time: u64,
        emergency_override_active: bool,
    ) -> FeeAdjustmentResult {
        let congestion_level = NetworkCongestionMonitor::get_current_congestion_level(metrics);
        
        // Get base multiplier from congestion level
        let mut congestion_multiplier = Self::get_multiplier_for_level(&congestion_level, config);
        
        // Calculate trend-based adjustment if enabled and we have previous metrics
        let mut trend_adjustment_bps = 0i32;
        let trend = if let Some(prev_metrics) = previous_metrics {
            let t = NetworkCongestionMonitor::calculate_trend(metrics, prev_metrics);
            
            if config.enable_trend_adjustment {
                match t {
                    CongestionTrend::Increasing => {
                        // Apply additional fee increase for rising congestion
                        let adjustment_percent = config.trend_adjustment_percent.min(50);
                        trend_adjustment_bps = Self::calculate_trend_adjustment(
                            config.base_fee_bps,
                            adjustment_percent,
                            true,
                        ) as i32;
                        // Boost multiplier slightly for increasing trend
                        congestion_multiplier = congestion_multiplier.saturating_add(5);
                    }
                    CongestionTrend::Decreasing => {
                        // Apply slight fee decrease for falling congestion
                        let adjustment_percent = config.trend_adjustment_percent.min(50);
                        trend_adjustment_bps = -(Self::calculate_trend_adjustment(
                            config.base_fee_bps,
                            adjustment_percent,
                            false,
                        ) as i32);
                        // Reduce multiplier slightly for decreasing trend
                        congestion_multiplier = congestion_multiplier.saturating_sub(3);
                    }
                    CongestionTrend::Stable => {
                        // No trend adjustment for stable congestion
                        trend_adjustment_bps = 0;
                    }
                }
            }
            t
        } else {
            CongestionTrend::Stable
        };

        // Calculate base adjusted fee
        let mut adjusted_fee = Self::apply_multiplier(config.base_fee_bps, congestion_multiplier);
        
        // Apply trend adjustment
        if trend_adjustment_bps > 0 {
            adjusted_fee = adjusted_fee.saturating_add(trend_adjustment_bps as u32);
        } else if trend_adjustment_bps < 0 {
            adjusted_fee = adjusted_fee.saturating_sub((-trend_adjustment_bps) as u32);
        }

        // Apply emergency override if active and enabled
        if emergency_override_active && config.enable_emergency_override {
            adjusted_fee = adjusted_fee.min(config.emergency_fee_cap_bps);
        }

        // Apply hard limits
        adjusted_fee = adjusted_fee.max(config.min_fee_bps).min(config.max_fee_bps);

        FeeAdjustmentResult {
            adjusted_fee_bps: adjusted_fee,
            base_fee_bps: config.base_fee_bps,
            congestion_multiplier,
            trend_adjustment_bps,
            congestion_level,
            trend,
            emergency_override_active,
            calculated_at: current_time,
        }
    }

    /// Get multiplier for a specific congestion level
    fn get_multiplier_for_level(level: &CongestionLevel, config: &FeeAdjustmentConfig) -> u32 {
        match level {
            CongestionLevel::VeryLow => config.very_low_multiplier,
            CongestionLevel::Low => config.low_multiplier,
            CongestionLevel::Moderate => config.moderate_multiplier,
            CongestionLevel::High => config.high_multiplier,
            CongestionLevel::Critical => config.critical_multiplier,
        }
    }

    /// Apply a multiplier to a fee amount
    /// multiplier uses fixed-point: 100 = 1.0, 200 = 2.0, etc.
    fn apply_multiplier(fee: u32, multiplier: u32) -> u32 {
        ((fee as u64 * multiplier as u64) / Self::MULTIPLIER_PRECISION as u64) as u32
    }

    /// Calculate trend adjustment amount in basis points
    fn calculate_trend_adjustment(base_fee: u32, percent: u32, is_increase: bool) -> u32 {
        let adjustment = ((base_fee as u64 * percent as u64) / 100) as u32;
        if is_increase {
            adjustment
        } else {
            adjustment / 2  // Less aggressive on decreases
        }
    }

    /// Check if fee update is allowed (respects cooldown)
    pub fn can_update_fee(last_update_time: u64, current_time: u64, cooldown_seconds: u64) -> bool {
        (current_time - last_update_time) >= cooldown_seconds
    }

    /// Validate that an emergency override should be triggered based on severity
    pub fn should_trigger_emergency_override(
        current_level: CongestionLevel,
        current_metrics: &NetworkMetrics,
    ) -> bool {
        // Emergency override triggers at critical congestion AND high gas prices
        match current_level {
            CongestionLevel::Critical => {
                current_metrics.avg_gas_price > 3000 || current_metrics.capacity_utilization_percent >= 95
            }
            _ => false,
        }
    }

    /// Calculate fee impact on user experience
    pub fn calculate_fee_impact(
        original_fee_bps: u32,
        adjusted_fee_bps: u32,
        transaction_amount: u64,
    ) -> FeeImpact {
        let original_fee_amount = (transaction_amount as u128 * original_fee_bps as u128) / 10000;
        let adjusted_fee_amount = (transaction_amount as u128 * adjusted_fee_bps as u128) / 10000;
        
        let fee_difference = if adjusted_fee_amount > original_fee_amount {
            (adjusted_fee_amount - original_fee_amount) as u64
        } else {
            0
        };

        let fee_difference_percent = if original_fee_amount > 0 {
            ((adjusted_fee_amount - original_fee_amount) as i128 * 100) / (original_fee_amount as i128)
        } else {
            0
        };

        FeeImpact {
            original_fee: original_fee_amount as u64,
            adjusted_fee: adjusted_fee_amount as u64,
            fee_difference,
            fee_difference_percent: fee_difference_percent as i32,
        }
    }

    /// Estimate optimal fee for a given congestion recovery scenario
    pub fn estimate_optimal_fee(
        config: &FeeAdjustmentConfig,
        current_level: CongestionLevel,
        target_level: CongestionLevel,
    ) -> u32 {
        let current_multiplier = Self::get_multiplier_for_level(&current_level, config);
        let target_multiplier = Self::get_multiplier_for_level(&target_level, config);
        
        // Average between current and target for smoother transition
        let avg_multiplier = (current_multiplier + target_multiplier) / 2;
        Self::apply_multiplier(config.base_fee_bps, avg_multiplier)
    }
}

/// Fee impact metrics
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeImpact {
    /// Original fee amount (in stroops)
    pub original_fee: u64,
    
    /// Adjusted fee amount (in stroops)
    pub adjusted_fee: u64,
    
    /// Absolute difference in fee
    pub fee_difference: u64,
    
    /// Percentage change in fee
    pub fee_difference_percent: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> FeeAdjustmentConfig {
        FeeAdjustmentConfig::default()
    }

    fn create_test_metrics(capacity_utilization: u32, gas_price: u64) -> NetworkMetrics {
        NetworkMetrics {
            txn_volume_tps: 100,
            avg_gas_price: gas_price,
            pending_txn_count: 100,
            ledger_close_time: 1000,
            capacity_utilization_percent: capacity_utilization,
            timestamp: 1000,
            avg_confirmation_time_ms: 2000,
        }
    }

    #[test]
    fn test_apply_multiplier() {
        // Test 1.0x multiplier
        assert_eq!(DynamicFeeAdjustment::apply_multiplier(100, 100), 100);
        
        // Test 2.0x multiplier
        assert_eq!(DynamicFeeAdjustment::apply_multiplier(100, 200), 200);
        
        // Test 0.5x multiplier
        assert_eq!(DynamicFeeAdjustment::apply_multiplier(100, 50), 50);
    }

    #[test]
    fn test_can_update_fee_with_cooldown() {
        assert!(DynamicFeeAdjustment::can_update_fee(1000, 1100, 60));
        assert!(!DynamicFeeAdjustment::can_update_fee(1000, 1030, 60));
        assert!(DynamicFeeAdjustment::can_update_fee(1000, 1060, 60));
    }

    #[test]
    fn test_calculate_adjusted_fee_very_low_congestion() {
        let config = create_test_config();
        let metrics = create_test_metrics(10, 100); // Very low congestion
        
        let result = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &metrics,
            None,
            1000,
            false,
        );
        
        // 50 bps * 0.8 = 40 bps
        assert_eq!(result.adjusted_fee_bps, 40);
        assert_eq!(result.congestion_level, CongestionLevel::VeryLow);
    }

    #[test]
    fn test_calculate_adjusted_fee_critical_congestion() {
        let config = create_test_config();
        let metrics = create_test_metrics(100, 5000); // Critical congestion
        
        let result = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &metrics,
            None,
            1000,
            false,
        );
        
        // 50 bps * 2.5 = 125 bps, but capped at max_fee_bps (500)
        assert_eq!(result.adjusted_fee_bps, 125);
        assert_eq!(result.congestion_level, CongestionLevel::Critical);
    }

    #[test]
    fn test_calculate_adjusted_fee_respects_max_cap() {
        let mut config = create_test_config();
        config.max_fee_bps = 100;
        let metrics = create_test_metrics(100, 5000);
        
        let result = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &metrics,
            None,
            1000,
            false,
        );
        
        // Should be capped at max_fee_bps
        assert!(result.adjusted_fee_bps <= config.max_fee_bps);
    }

    #[test]
    fn test_calculate_adjusted_fee_respects_min_floor() {
        let config = create_test_config();
        let metrics = create_test_metrics(5, 100); // Very low congestion
        
        let result = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &metrics,
            None,
            1000,
            false,
        );
        
        // Should be at least min_fee_bps
        assert!(result.adjusted_fee_bps >= config.min_fee_bps);
    }

    #[test]
    fn test_emergency_override_caps_fees() {
        let config = create_test_config();
        let metrics = create_test_metrics(100, 5000);
        
        let result_no_override = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &metrics,
            None,
            1000,
            false,
        );
        
        let result_with_override = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &metrics,
            None,
            1000,
            true,
        );
        
        // With override, fee should not exceed emergency cap
        assert!(result_with_override.adjusted_fee_bps <= config.emergency_fee_cap_bps);
        assert!(result_with_override.emergency_override_active);
    }

    #[test]
    fn test_calculate_fee_impact() {
        let impact = DynamicFeeAdjustment::calculate_fee_impact(50, 125, 1_000_000);
        
        // Original: 1,000,000 * 50 / 10000 = 5,000 stroops
        // Adjusted: 1,000,000 * 125 / 10000 = 12,500 stroops
        // Difference: 7,500 stroops (150% increase)
        assert_eq!(impact.original_fee, 5000);
        assert_eq!(impact.adjusted_fee, 12500);
        assert_eq!(impact.fee_difference, 7500);
        assert_eq!(impact.fee_difference_percent, 150);
    }

    #[test]
    fn test_should_trigger_emergency_override() {
        let metrics_critical_high_gas = NetworkMetrics {
            txn_volume_tps: 1000,
            avg_gas_price: 4000,
            pending_txn_count: 10000,
            ledger_close_time: 1000,
            capacity_utilization_percent: 90,
            timestamp: 1000,
            avg_confirmation_time_ms: 10000,
        };
        
        assert!(DynamicFeeAdjustment::should_trigger_emergency_override(
            CongestionLevel::Critical,
            &metrics_critical_high_gas
        ));
    }

    #[test]
    fn test_should_not_trigger_emergency_override_at_high_only() {
        let metrics_high = NetworkMetrics {
            txn_volume_tps: 600,
            avg_gas_price: 2000,
            pending_txn_count: 5000,
            ledger_close_time: 1000,
            capacity_utilization_percent: 70,
            timestamp: 1000,
            avg_confirmation_time_ms: 5000,
        };
        
        assert!(!DynamicFeeAdjustment::should_trigger_emergency_override(
            CongestionLevel::High,
            &metrics_high
        ));
    }
}
