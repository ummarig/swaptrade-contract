use soroban_sdk::{contracttype, Env, Symbol, symbol_short, Address};
use crate::network_congestion::{CongestionLevel, NetworkMetrics, NetworkCongestionMonitor};

/// Emergency override status
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum OverrideStatus {
    /// Override is not active
    Inactive,
    
    /// Override is currently active
    Active,
    
    /// Override is temporarily suspended (recovery phase)
    Suspended,
}

/// Reason for activating emergency override
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum OverrideReason {
    /// Critical congestion detected
    CriticalCongestion,
    
    /// Extreme gas prices
    ExtremeGasPrices,
    
    /// Network queue overflow
    QueueOverflow,
    
    /// Manual administrator intervention
    ManualIntervention,
    
    /// Multiple congestion indicators exceeded
    MultipleTriggers,
}

/// Emergency override state information
#[derive(Clone, Debug)]
#[contracttype]
pub struct EmergencyOverrideState {
    /// Whether override is currently active
    pub status: OverrideStatus,
    
    /// Fee cap in basis points during override
    pub fee_cap_bps: u32,
    
    /// When override was activated
    pub activated_at: u64,
    
    /// When override should auto-deactivate (if any)
    pub auto_deactivate_at: Option<u64>,
    
    /// Reason for activation
    pub reason: OverrideReason,
    
    /// Admin who activated (if manual)
    pub activated_by: Option<Address>,
    
    /// Number of times emergency override has been triggered
    pub trigger_count: u32,
    
    /// When override was last toggled
    pub last_status_change: u64,
}

/// Emergency override manager for protecting network during extreme congestion
pub struct EmergencyOverrideManager;

impl EmergencyOverrideManager {
    /// Storage key for emergency override state
    pub const OVERRIDE_STATE_KEY: Symbol = symbol_short!("emgovst");
    
    /// Storage key for override history
    pub const OVERRIDE_HISTORY_KEY: Symbol = symbol_short!("emgohst");
    
    /// Storage key for override authorization list
    pub const AUTHORIZED_ADMINS_KEY: Symbol = symbol_short!("emgoaut");
    
    /// Default emergency fee cap (basis points)
    const DEFAULT_EMERGENCY_FEE_CAP: u32 = 300; // 3.0%
    
    /// Auto-recovery timeout for auto-deactivation (in seconds)
    const AUTO_RECOVERY_TIMEOUT: u64 = 1800; // 30 minutes
    
    /// Critical capacity threshold for auto-trigger
    const CRITICAL_CAPACITY_THRESHOLD: u32 = 95;
    
    /// Critical gas price threshold for auto-trigger
    const CRITICAL_GAS_PRICE_THRESHOLD: u64 = 5000;
    
    /// Critical pending transaction threshold for auto-trigger
    const CRITICAL_PENDING_TXN_THRESHOLD: u64 = 15000;

    /// Initialize emergency override system
    pub fn initialize(env: &Env) {
        let state = EmergencyOverrideState {
            status: OverrideStatus::Inactive,
            fee_cap_bps: Self::DEFAULT_EMERGENCY_FEE_CAP,
            activated_at: 0,
            auto_deactivate_at: None,
            reason: OverrideReason::CriticalCongestion,
            activated_by: None,
            trigger_count: 0,
            last_status_change: 0,
        };
        
        env.storage()
            .persistent()
            .set(&Self::OVERRIDE_STATE_KEY, &state);
    }

    /// Get current override state
    pub fn get_state(env: &Env) -> EmergencyOverrideState {
        env.storage()
            .persistent()
            .get(&Self::OVERRIDE_STATE_KEY)
            .unwrap_or_else(|| EmergencyOverrideState {
                status: OverrideStatus::Inactive,
                fee_cap_bps: Self::DEFAULT_EMERGENCY_FEE_CAP,
                activated_at: 0,
                auto_deactivate_at: None,
                reason: OverrideReason::CriticalCongestion,
                activated_by: None,
                trigger_count: 0,
                last_status_change: 0,
            })
    }

    /// Check if emergency override is currently active
    pub fn is_active(env: &Env) -> bool {
        let state = Self::get_state(env);
        state.status == OverrideStatus::Active
    }

    /// Automatically detect if emergency override should be triggered
    pub fn should_auto_trigger(metrics: &NetworkMetrics) -> (bool, Option<OverrideReason>) {
        let mut trigger_count = 0;
        let mut primary_reason = None;

        // Check capacity utilization
        if metrics.capacity_utilization_percent >= Self::CRITICAL_CAPACITY_THRESHOLD {
            trigger_count += 1;
            if primary_reason.is_none() {
                primary_reason = Some(OverrideReason::CriticalCongestion);
            }
        }

        // Check gas prices
        if metrics.avg_gas_price >= Self::CRITICAL_GAS_PRICE_THRESHOLD {
            trigger_count += 1;
            if primary_reason.is_none() {
                primary_reason = Some(OverrideReason::ExtremeGasPrices);
            }
        }

        // Check pending transaction queue
        if metrics.pending_txn_count >= Self::CRITICAL_PENDING_TXN_THRESHOLD {
            trigger_count += 1;
            if primary_reason.is_none() {
                primary_reason = Some(OverrideReason::QueueOverflow);
            }
        }

        // Trigger if multiple indicators exceed threshold
        let should_trigger = if trigger_count >= 2 {
            true
        } else if trigger_count == 1 {
            // Single trigger only if extremely critical
            metrics.capacity_utilization_percent >= 98
                || metrics.avg_gas_price >= 7000
                || metrics.pending_txn_count >= 20000
        } else {
            false
        };

        let reason = if trigger_count >= 2 {
            Some(OverrideReason::MultipleTriggers)
        } else {
            primary_reason
        };

        (should_trigger, reason)
    }

    /// Manually activate emergency override (admin only)
    pub fn activate_manual(
        env: &Env,
        admin: Address,
        fee_cap_bps: u32,
        current_time: u64,
    ) -> Result<(), String> {
        // Verify admin is authorized
        if !Self::is_authorized_admin(env, admin.clone()) {
            return Err("Unauthorized admin".to_string());
        }

        let mut state = Self::get_state(env);
        
        if state.status == OverrideStatus::Active {
            return Err("Override already active".to_string());
        }

        state.status = OverrideStatus::Active;
        state.fee_cap_bps = fee_cap_bps;
        state.activated_at = current_time;
        state.auto_deactivate_at = Some(current_time + Self::AUTO_RECOVERY_TIMEOUT);
        state.reason = OverrideReason::ManualIntervention;
        state.activated_by = Some(admin);
        state.trigger_count = state.trigger_count.saturating_add(1);
        state.last_status_change = current_time;

        env.storage()
            .persistent()
            .set(&Self::OVERRIDE_STATE_KEY, &state);

        Ok(())
    }

    /// Automatically activate emergency override based on metrics
    pub fn activate_automatic(
        env: &Env,
        reason: OverrideReason,
        metrics: &NetworkMetrics,
        current_time: u64,
    ) -> Result<(), String> {
        let mut state = Self::get_state(env);
        
        if state.status == OverrideStatus::Active {
            return Err("Override already active".to_string());
        }

        // Calculate fee cap based on severity
        let fee_cap = match reason {
            OverrideReason::ExtremeGasPrices => 250, // 2.5%
            OverrideReason::QueueOverflow => 280,    // 2.8%
            OverrideReason::MultipleTriggers => 200, // 2.0%
            OverrideReason::CriticalCongestion => 300, // 3.0%
            _ => Self::DEFAULT_EMERGENCY_FEE_CAP,
        };

        state.status = OverrideStatus::Active;
        state.fee_cap_bps = fee_cap;
        state.activated_at = current_time;
        state.auto_deactivate_at = Some(current_time + Self::AUTO_RECOVERY_TIMEOUT);
        state.reason = reason;
        state.activated_by = None; // System-triggered
        state.trigger_count = state.trigger_count.saturating_add(1);
        state.last_status_change = current_time;

        env.storage()
            .persistent()
            .set(&Self::OVERRIDE_STATE_KEY, &state);

        Ok(())
    }

    /// Deactivate emergency override (manual or automatic)
    pub fn deactivate(
        env: &Env,
        admin: Option<Address>,
        current_time: u64,
    ) -> Result<(), String> {
        let mut state = Self::get_state(env);
        
        if state.status != OverrideStatus::Active {
            return Err("Override not active".to_string());
        }

        // Check authorization for manual deactivation
        if let Some(admin_addr) = admin {
            if !Self::is_authorized_admin(env, admin_addr) {
                return Err("Unauthorized admin".to_string());
            }
        }

        state.status = OverrideStatus::Inactive;
        state.auto_deactivate_at = None;
        state.last_status_change = current_time;

        env.storage()
            .persistent()
            .set(&Self::OVERRIDE_STATE_KEY, &state);

        Ok(())
    }

    /// Check if override should auto-deactivate due to timeout
    pub fn check_auto_deactivation(env: &Env, current_time: u64) -> bool {
        let mut state = Self::get_state(env);
        
        if state.status != OverrideStatus::Active {
            return false;
        }

        if let Some(deactivate_at) = state.auto_deactivate_at {
            if current_time >= deactivate_at {
                state.status = OverrideStatus::Inactive;
                state.auto_deactivate_at = None;
                state.last_status_change = current_time;
                
                env.storage()
                    .persistent()
                    .set(&Self::OVERRIDE_STATE_KEY, &state);
                
                return true;
            }
        }

        false
    }

    /// Check if network is recovering (for early deactivation consideration)
    pub fn is_network_recovering(
        current_metrics: &NetworkMetrics,
        previous_metrics: Option<&NetworkMetrics>,
    ) -> bool {
        if let Some(prev) = previous_metrics {
            let trend = NetworkCongestionMonitor::calculate_trend(current_metrics, prev);
            let recovery_score = (
                (current_metrics.capacity_utilization_percent < 70) as u32
                + (current_metrics.avg_gas_price < 2000) as u32
                + (current_metrics.pending_txn_count < 5000) as u32
            );
            
            trend == crate::network_congestion::CongestionTrend::Decreasing && recovery_score >= 2
        } else {
            false
        }
    }

    /// Add admin authorization
    pub fn add_authorized_admin(env: &Env, admin: Address) {
        let mut admins: soroban_sdk::Vec<Address> = env
            .storage()
            .persistent()
            .get(&Self::AUTHORIZED_ADMINS_KEY)
            .unwrap_or_else(|| soroban_sdk::Vec::new(env));

        // Check if already authorized
        let mut already_exists = false;
        for existing_admin in admins.iter() {
            if existing_admin == admin {
                already_exists = true;
                break;
            }
        }

        if !already_exists {
            admins.push_back(admin);
            env.storage()
                .persistent()
                .set(&Self::AUTHORIZED_ADMINS_KEY, &admins);
        }
    }

    /// Remove admin authorization
    pub fn remove_authorized_admin(env: &Env, admin: Address) {
        let mut admins: soroban_sdk::Vec<Address> = env
            .storage()
            .persistent()
            .get(&Self::AUTHORIZED_ADMINS_KEY)
            .unwrap_or_else(|| soroban_sdk::Vec::new(env));

        let mut new_admins = soroban_sdk::Vec::new(env);
        for existing_admin in admins.iter() {
            if existing_admin != admin {
                new_admins.push_back(existing_admin);
            }
        }

        env.storage()
            .persistent()
            .set(&Self::AUTHORIZED_ADMINS_KEY, &new_admins);
    }

    /// Check if admin is authorized for emergency overrides
    pub fn is_authorized_admin(env: &Env, admin: Address) -> bool {
        let admins: soroban_sdk::Vec<Address> = env
            .storage()
            .persistent()
            .get(&Self::AUTHORIZED_ADMINS_KEY)
            .unwrap_or_else(|| soroban_sdk::Vec::new(env));

        for existing_admin in admins.iter() {
            if existing_admin == admin {
                return true;
            }
        }
        false
    }

    /// Get time remaining until auto-deactivation
    pub fn get_time_until_auto_deactivation(env: &Env, current_time: u64) -> Option<u64> {
        let state = Self::get_state(env);
        
        if state.status != OverrideStatus::Active {
            return None;
        }

        state.auto_deactivate_at.and_then(|deactivate_at| {
            if deactivate_at > current_time {
                Some(deactivate_at - current_time)
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_override_status_equality() {
        assert_eq!(OverrideStatus::Active, OverrideStatus::Active);
        assert_ne!(OverrideStatus::Active, OverrideStatus::Inactive);
    }

    #[test]
    fn test_override_reason_equality() {
        assert_eq!(
            OverrideReason::CriticalCongestion,
            OverrideReason::CriticalCongestion
        );
        assert_ne!(
            OverrideReason::CriticalCongestion,
            OverrideReason::ExtremeGasPrices
        );
    }

    #[test]
    fn test_should_auto_trigger_no_trigger() {
        let metrics = NetworkMetrics {
            txn_volume_tps: 100,
            avg_gas_price: 500,
            pending_txn_count: 1000,
            ledger_close_time: 1000,
            capacity_utilization_percent: 50,
            timestamp: 1000,
            avg_confirmation_time_ms: 2000,
        };

        let (should_trigger, _reason) = EmergencyOverrideManager::should_auto_trigger(&metrics);
        assert!(!should_trigger);
    }

    #[test]
    fn test_should_auto_trigger_multiple_indicators() {
        let metrics = NetworkMetrics {
            txn_volume_tps: 1000,
            avg_gas_price: 6000,
            pending_txn_count: 20000,
            ledger_close_time: 1000,
            capacity_utilization_percent: 98,
            timestamp: 1000,
            avg_confirmation_time_ms: 10000,
        };

        let (should_trigger, reason) = EmergencyOverrideManager::should_auto_trigger(&metrics);
        assert!(should_trigger);
        assert_eq!(reason, Some(OverrideReason::MultipleTriggers));
    }

    #[test]
    fn test_should_auto_trigger_extreme_capacity() {
        let metrics = NetworkMetrics {
            txn_volume_tps: 800,
            avg_gas_price: 2000,
            pending_txn_count: 8000,
            ledger_close_time: 1000,
            capacity_utilization_percent: 99, // Extremely high
            timestamp: 1000,
            avg_confirmation_time_ms: 8000,
        };

        let (should_trigger, reason) = EmergencyOverrideManager::should_auto_trigger(&metrics);
        assert!(should_trigger);
    }
}
