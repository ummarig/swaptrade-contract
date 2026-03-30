use soroban_sdk::{contracttype, Env, Address, Symbol, symbol_short};
use crate::network_congestion::{NetworkMetrics, NetworkCongestionMonitor, CongestionLevel};
use crate::dynamic_fee_adjustment::{DynamicFeeAdjustment, FeeAdjustmentConfig, FeeAdjustmentResult};
use crate::fee_history::{FeeHistoryManager, AdjustmentReason};
use crate::emergency_override::{EmergencyOverrideManager, OverrideReason};

/// Main contract interface for trading fee adjustment
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeAdjustmentInfo {
    /// Current trading fee in basis points
    pub current_fee_bps: u32,
    
    /// Previous trading fee
    pub previous_fee_bps: u32,
    
    /// Current congestion level
    pub congestion_level: CongestionLevel,
    
    /// Fee adjustment configuration
    pub config: FeeAdjustmentConfig,
    
    /// Whether emergency override is active
    pub emergency_override_active: bool,
    
    /// Current network metrics
    pub network_metrics: NetworkMetrics,
    
    /// Last update timestamp
    pub last_update: u64,
}

/// Fee adjustment manager - orchestrates all fee adjustment components
pub struct FeeAdjustmentManager;

impl FeeAdjustmentManager {
    /// Storage key for current trading fee
    const CURRENT_FEE_KEY: Symbol = symbol_short!("curfee");
    
    /// Storage key for previous trading fee
    const PREVIOUS_FEE_KEY: Symbol = symbol_short!("prvfee");
    
    /// Storage key for fee adjustment config
    const FEE_CONFIG_KEY: Symbol = symbol_short!("feecfg");
    
    /// Storage key for current network metrics
    const NETWORK_METRICS_KEY: Symbol = symbol_short!("netmet");
    
    /// Storage key for previous network metrics (for trend calculation)
    const PREVIOUS_METRICS_KEY: Symbol = symbol_short!("prvmet");

    /// Initialize the fee adjustment system
    pub fn initialize(env: &Env, config: FeeAdjustmentConfig, initial_metrics: NetworkMetrics) {
        // Store initial configuration
        env.storage()
            .persistent()
            .set(&Self::FEE_CONFIG_KEY, &config);

        // Store initial fees
        env.storage()
            .persistent()
            .set(&Self::CURRENT_FEE_KEY, &config.base_fee_bps);
        
        env.storage()
            .persistent()
            .set(&Self::PREVIOUS_FEE_KEY, &config.base_fee_bps);

        // Store initial metrics
        env.storage()
            .persistent()
            .set(&Self::NETWORK_METRICS_KEY, &initial_metrics);

        // Initialize emergency override system
        EmergencyOverrideManager::initialize(env);

        // Log initial recording in history
        let result = FeeAdjustmentResult {
            adjusted_fee_bps: config.base_fee_bps,
            base_fee_bps: config.base_fee_bps,
            congestion_multiplier: 100,
            trend_adjustment_bps: 0,
            congestion_level: NetworkCongestionMonitor::get_current_congestion_level(&initial_metrics),
            trend: crate::network_congestion::CongestionTrend::Stable,
            emergency_override_active: false,
            calculated_at: env.ledger().timestamp(),
        };

        FeeHistoryManager::record_fee_adjustment(
            env,
            &result,
            config.base_fee_bps,
            &initial_metrics,
            symbol_short!("init"),
            AdjustmentReason::SystemInitialization,
        );
    }

    /// Update fees based on current network conditions
    pub fn update_fees_for_congestion(
        env: &Env,
        current_metrics: NetworkMetrics,
    ) -> Result<FeeAdjustmentResult, String> {
        let current_time = env.ledger().timestamp();
        let config = Self::get_config(env)?;

        // Check cooldown period
        let last_update = env
            .storage()
            .persistent()
            .get::<Symbol, u64>(&symbol_short!("lstupd"))
            .unwrap_or(0);

        if !DynamicFeeAdjustment::can_update_fee(last_update, current_time, config.update_cooldown_seconds) {
            return Err("Update cooldown not elapsed".to_string());
        }

        // Get previous metrics for trend calculation
        let previous_metrics: Option<NetworkMetrics> = env
            .storage()
            .persistent()
            .get(&Self::PREVIOUS_METRICS_KEY);

        // Check if emergency override should be automatically triggered
        let emergency_override_active = EmergencyOverrideManager::is_active(env);
        let (should_auto_trigger, auto_trigger_reason) = 
            EmergencyOverrideManager::should_auto_trigger(&current_metrics);

        let mut new_emergency_active = emergency_override_active;

        if should_auto_trigger && !emergency_override_active {
            if let Some(reason) = auto_trigger_reason {
                let _ = EmergencyOverrideManager::activate_automatic(env, reason, &current_metrics, current_time);
                new_emergency_active = true;
                
                // Emit event for emergency activation
                crate::events::emergency_fee_override_activated(
                    env,
                    config.emergency_fee_cap_bps,
                    symbol_short!("auto"),
                    current_time as u64,
                );
            }
        }

        // Check if emergency override should auto-deactivate
        if emergency_override_active {
            EmergencyOverrideManager::check_auto_deactivation(env, current_time);
            new_emergency_active = EmergencyOverrideManager::is_active(env);
        }

        // Calculate new adjusted fee
        let result = DynamicFeeAdjustment::calculate_adjusted_fee(
            &config,
            &current_metrics,
            previous_metrics.as_ref(),
            current_time as u64,
            new_emergency_active,
        );

        let previous_fee = Self::get_current_fee(env).unwrap_or(config.base_fee_bps);

        // Only update if fee actually changed
        if result.adjusted_fee_bps != previous_fee {
            // Update stored fees
            env.storage()
                .persistent()
                .set(&Self::PREVIOUS_FEE_KEY, &previous_fee);
            
            env.storage()
                .persistent()
                .set(&Self::CURRENT_FEE_KEY, &result.adjusted_fee_bps);

            // Record in history
            FeeHistoryManager::record_fee_adjustment(
                env,
                &result,
                previous_fee,
                &current_metrics,
                symbol_short!("auto"),
                AdjustmentReason::AutomaticCongestionAdjustment,
            );

            // Emit event
            let congestion_tag = match result.congestion_level {
                CongestionLevel::VeryLow => symbol_short!("vlowd"),
                CongestionLevel::Low => symbol_short!("low"),
                CongestionLevel::Moderate => symbol_short!("mod"),
                CongestionLevel::High => symbol_short!("high"),
                CongestionLevel::Critical => symbol_short!("crit"),
            };

            crate::events::fee_adjustment_applied(
                env,
                previous_fee,
                result.adjusted_fee_bps,
                symbol_short!("auto"),
                congestion_tag,
                current_time as u64,
            );
        }

        // Update metrics for next iteration
        env.storage()
            .persistent()
            .set(&Self::PREVIOUS_METRICS_KEY, &current_metrics);
        
        env.storage()
            .persistent()
            .set(&Self::NETWORK_METRICS_KEY, &current_metrics);

        // Update last update timestamp
        env.storage()
            .persistent()
            .set(&symbol_short!("lstupd"), &current_time);

        Ok(result)
    }

    /// Manually update trading fees (admin only)
    pub fn update_fees_manual(
        env: &Env,
        admin: Address,
        new_fee_bps: u32,
    ) -> Result<(), String> {
        // Admin authorization would be checked through require_auth in contract
        let config = Self::get_config(env)?;
        let current_time = env.ledger().timestamp();

        // Validate fee is within bounds
        if new_fee_bps < config.min_fee_bps || new_fee_bps > config.max_fee_bps {
            return Err("Fee out of bounds".to_string());
        }

        let previous_fee = Self::get_current_fee(env).unwrap_or(config.base_fee_bps);

        if new_fee_bps == previous_fee {
            return Err("Fee unchanged".to_string());
        }

        // Update fee
        env.storage()
            .persistent()
            .set(&Self::PREVIOUS_FEE_KEY, &previous_fee);
        
        env.storage()
            .persistent()
            .set(&Self::CURRENT_FEE_KEY, &new_fee_bps);

        // Get current metrics for history
        let metrics = Self::get_network_metrics(env).unwrap_or_else(|| NetworkMetrics {
            txn_volume_tps: 0,
            avg_gas_price: 0,
            pending_txn_count: 0,
            ledger_close_time: current_time,
            capacity_utilization_percent: 0,
            timestamp: current_time as u64,
            avg_confirmation_time_ms: 0,
        });

        let result = FeeAdjustmentResult {
            adjusted_fee_bps: new_fee_bps,
            base_fee_bps: config.base_fee_bps,
            congestion_multiplier: 100,
            trend_adjustment_bps: 0,
            congestion_level: NetworkCongestionMonitor::get_current_congestion_level(&metrics),
            trend: crate::network_congestion::CongestionTrend::Stable,
            emergency_override_active: EmergencyOverrideManager::is_active(env),
            calculated_at: current_time as u64,
        };

        // Record in history
        FeeHistoryManager::record_fee_adjustment(
            env,
            &result,
            previous_fee,
            &metrics,
            admin,
            AdjustmentReason::ManualAdminAdjustment,
        );

        // Emit event
        crate::events::fee_adjustment_applied(
            env,
            previous_fee,
            new_fee_bps,
            symbol_short!("admin"),
            symbol_short!("man"),
            current_time as u64,
        );

        Ok(())
    }

    /// Update fee adjustment configuration
    pub fn update_config(
        env: &Env,
        _admin: Address,
        new_config: FeeAdjustmentConfig,
    ) -> Result<(), String> {
        // Admin authorization would be checked through require_auth in contract

        // Validate configuration
        if new_config.min_fee_bps > new_config.base_fee_bps {
            return Err("Min fee exceeds base fee".to_string());
        }

        if new_config.base_fee_bps > new_config.max_fee_bps {
            return Err("Base fee exceeds max fee".to_string());
        }

        if new_config.max_fee_bps > 1000 {
            return Err("Max fee exceeds 10%".to_string());
        }

        env.storage()
            .persistent()
            .set(&Self::FEE_CONFIG_KEY, &new_config);

        let current_time = env.ledger().timestamp();
        crate::events::fee_configuration_updated(
            env,
            _admin,
            symbol_short!("config"),
            current_time as u64,
        );

        Ok(())
    }

    /// Get current trading fee
    pub fn get_current_fee(env: &Env) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&Self::CURRENT_FEE_KEY)
    }

    /// Get previous trading fee
    pub fn get_previous_fee(env: &Env) -> Option<u32> {
        env.storage()
            .persistent()
            .get(&Self::PREVIOUS_FEE_KEY)
    }

    /// Get fee adjustment configuration
    pub fn get_config(env: &Env) -> Result<FeeAdjustmentConfig, String> {
        env.storage()
            .persistent()
            .get(&Self::FEE_CONFIG_KEY)
            .ok_or("Configuration not initialized".to_string())
    }

    /// Get current network metrics
    pub fn get_network_metrics(env: &Env) -> Option<NetworkMetrics> {
        env.storage()
            .persistent()
            .get(&Self::NETWORK_METRICS_KEY)
    }

    /// Get complete fee adjustment info
    pub fn get_fee_adjustment_info(env: &Env) -> Result<FeeAdjustmentInfo, String> {
        let config = Self::get_config(env)?;
        let current_fee = Self::get_current_fee(env).ok_or("Current fee not set".to_string())?;
        let previous_fee = Self::get_previous_fee(env).unwrap_or(config.base_fee_bps);
        let metrics = Self::get_network_metrics(env).ok_or("Metrics not available".to_string())?;

        Ok(FeeAdjustmentInfo {
            current_fee_bps: current_fee,
            previous_fee_bps: previous_fee,
            congestion_level: NetworkCongestionMonitor::get_current_congestion_level(&metrics),
            config,
            emergency_override_active: EmergencyOverrideManager::is_active(env),
            network_metrics: metrics,
            last_update: env
                .storage()
                .persistent()
                .get::<Symbol, u64>(&symbol_short!("lstupd"))
                .unwrap_or(0),
        })
    }

    /// Activate emergency override (admin only)
    pub fn activate_emergency_override(
        env: &Env,
        _admin: Address,
        fee_cap_bps: u32,
    ) -> Result<(), String> {
        // Admin authorization would be checked through require_auth in contract
        let current_time = env.ledger().timestamp();
        EmergencyOverrideManager::activate_manual(env, _admin, fee_cap_bps, current_time)?;

        crate::events::emergency_fee_override_activated(
            env,
            fee_cap_bps,
            symbol_short!("manual"),
            current_time as u64,
        );

        Ok(())
    }

    /// Deactivate emergency override (admin only)
    pub fn deactivate_emergency_override(
        env: &Env,
        admin: Address,
    ) -> Result<(), String> {
        // Admin authorization would be checked through require_auth in contract
        let current_time = env.ledger().timestamp();
        EmergencyOverrideManager::deactivate(env, Some(admin), current_time)?;

        crate::events::emergency_fee_override_deactivated(
            env,
            current_time as u64,
        );

        Ok(())
    }

    /// Add admin authorized for emergency overrides
    pub fn add_emergency_admin(env: &Env, admin: Address) {
        EmergencyOverrideManager::add_authorized_admin(env, admin);
    }

    /// Remove admin authorization for emergency overrides
    pub fn remove_emergency_admin(env: &Env, admin: Address) {
        EmergencyOverrideManager::remove_authorized_admin(env, admin);
    }

    /// Get emergency override state
    pub fn get_emergency_override_state(env: &Env) -> crate::emergency_override::EmergencyOverrideState {
        EmergencyOverrideManager::get_state(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_adjustment_info_creation() {
        // This test verifies the structure is valid
        let _info = FeeAdjustmentInfo {
            current_fee_bps: 50,
            previous_fee_bps: 45,
            congestion_level: CongestionLevel::VeryLow,
            config: FeeAdjustmentConfig::default(),
            emergency_override_active: false,
            network_metrics: NetworkMetrics {
                txn_volume_tps: 100,
                avg_gas_price: 100,
                pending_txn_count: 100,
                ledger_close_time: 1000,
                capacity_utilization_percent: 10,
                timestamp: 1000,
                avg_confirmation_time_ms: 2000,
            },
            last_update: 1000,
        };
    }
}
