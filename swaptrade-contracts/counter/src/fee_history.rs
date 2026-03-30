use soroban_sdk::{contracttype, Env, Symbol, symbol_short, Vec, Map, Address};
use crate::network_congestion::{CongestionLevel, NetworkMetrics};
use crate::dynamic_fee_adjustment::FeeAdjustmentResult;

/// Fee history entry recording a fee adjustment event
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeHistoryEntry {
    /// Timestamp when fee was set
    pub timestamp: u64,
    
    /// Fee in basis points
    pub fee_bps: u32,
    
    /// Previous fee in basis points
    pub previous_fee_bps: u32,
    
    /// Congestion level at time of adjustment
    pub congestion_level: CongestionLevel,
    
    /// Network metrics snapshot at time of adjustment
    pub network_metrics: NetworkMetrics,
    
    /// Reason for fee adjustment
    pub adjustment_reason: AdjustmentReason,
    
    /// Who triggered the adjustment (admin address or system)
    pub triggered_by: Symbol,
}

/// Reason for fee adjustment
#[derive(Clone, Debug, PartialEq, Eq)]
#[contracttype]
pub enum AdjustmentReason {
    /// Automatic adjustment due to congestion change
    AutomaticCongestionAdjustment,
    
    /// Manual adjustment by administrator
    ManualAdminAdjustment,
    
    /// Emergency override applied
    EmergencyOverride,
    
    /// Recovery from emergency override
    EmergencyRecovery,
    
    /// Scheduled maintenance adjustment
    ScheduledMaintenance,
    
    /// System initialization
    SystemInitialization,
}

/// Fee history statistics for analysis and reporting
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeHistoryStats {
    /// Average fee over the period (in basis points)
    pub avg_fee_bps: u32,
    
    /// Minimum fee in the period
    pub min_fee_bps: u32,
    
    /// Maximum fee in the period
    pub max_fee_bps: u32,
    
    /// Total number of fee adjustments
    pub adjustment_count: u32,
    
    /// Number of days in the analysis period
    pub period_days: u32,
    
    /// Standard deviation of fees (measure of volatility)
    pub fee_volatility: u32,
    
    /// Number of times emergency override was triggered
    pub emergency_override_count: u32,
    
    /// Time period covered (in seconds)
    pub period_seconds: u64,
}

/// Fee snapshot for a specific time
#[derive(Clone, Debug)]
#[contracttype]
pub struct FeeSnapshot {
    /// Timestamp of snapshot
    pub timestamp: u64,
    
    /// Fee at this time
    pub fee_bps: u32,
    
    /// Congestion level
    pub congestion_level: CongestionLevel,
}

/// Fee history manager for storing and retrieving fee adjustments
pub struct FeeHistoryManager;

impl FeeHistoryManager {
    /// Storage key for fee history log (Vec of FeeHistoryEntry)
    pub const HISTORY_LOG_KEY: Symbol = symbol_short!("fhstlog");
    
    /// Storage key for current fees by user (Map<Address, FeeSnapshot>)
    pub const USER_FEES_KEY: Symbol = symbol_short!("usrfees");
    
    /// Storage key for global fee statistics
    pub const STATS_KEY: Symbol = symbol_short!("fstats");
    
    /// Storage key for fee adjustment events buffer
    pub const EVENTS_BUFFER_KEY: Symbol = symbol_short!("fevtbuf");
    
    /// Maximum history entries to keep in memory (to limit storage)
    const MAX_HISTORY_ENTRIES: usize = 10000;
    
    /// Time window for statistics calculation (in seconds)
    const STATS_WINDOW_SECONDS: u64 = 86400; // 24 hours

    /// Record a fee adjustment in history
    pub fn record_fee_adjustment(
        env: &Env,
        result: &FeeAdjustmentResult,
        previous_fee_bps: u32,
        metrics: &NetworkMetrics,
        triggered_by: Symbol,
        reason: AdjustmentReason,
    ) {
        let entry = FeeHistoryEntry {
            timestamp: result.calculated_at,
            fee_bps: result.adjusted_fee_bps,
            previous_fee_bps,
            congestion_level: result.congestion_level.clone(),
            network_metrics: metrics.clone(),
            adjustment_reason: reason,
            triggered_by,
        };

        // Get existing history
        let mut history: Vec<FeeHistoryEntry> = env
            .storage()
            .persistent()
            .get(&Self::HISTORY_LOG_KEY)
            .unwrap_or_else(|| Vec::new(env));

        // Add new entry
        history.push_back(entry);

        // Maintain size limit by removing oldest entries if needed
        while history.len() > Self::MAX_HISTORY_ENTRIES {
            history.pop_front();
        }

        // Store updated history
        env.storage()
            .persistent()
            .set(&Self::HISTORY_LOG_KEY, &history);
    }

    /// Get fee history entries within a time range
    pub fn get_history_range(
        env: &Env,
        start_time: u64,
        end_time: u64,
    ) -> Vec<FeeHistoryEntry> {
        let history: Vec<FeeHistoryEntry> = env
            .storage()
            .persistent()
            .get(&Self::HISTORY_LOG_KEY)
            .unwrap_or_else(|| Vec::new(env));

        let mut result = Vec::new(env);
        for entry in history.iter() {
            if entry.timestamp >= start_time && entry.timestamp <= end_time {
                result.push_back(entry);
            }
        }
        result
    }

    /// Get the most recent fee history entries
    pub fn get_recent_history(env: &Env, limit: u32) -> Vec<FeeHistoryEntry> {
        let history: Vec<FeeHistoryEntry> = env
            .storage()
            .persistent()
            .get(&Self::HISTORY_LOG_KEY)
            .unwrap_or_else(|| Vec::new(env));

        let mut result = Vec::new(env);
        let start_idx = if (history.len() as u32) > limit {
            (history.len() as u32) - limit
        } else {
            0
        } as usize;

        for i in start_idx..history.len() {
            result.push_back(history.get(i as u32).unwrap());
        }
        result
    }

    /// Calculate fee statistics for a time period
    pub fn calculate_statistics(env: &Env, period_seconds: u64, current_time: u64) -> FeeHistoryStats {
        let start_time = current_time.saturating_sub(period_seconds);
        let history = Self::get_history_range(env, start_time, current_time);

        if history.is_empty() {
            return FeeHistoryStats {
                avg_fee_bps: 0,
                min_fee_bps: 0,
                max_fee_bps: 0,
                adjustment_count: 0,
                period_days: (period_seconds / 86400) as u32,
                fee_volatility: 0,
                emergency_override_count: 0,
                period_seconds,
            };
        }

        let mut total_fee: u64 = 0;
        let mut min_fee = u32::MAX;
        let mut max_fee = 0u32;
        let mut emergency_count = 0u32;

        for entry in history.iter() {
            total_fee += entry.fee_bps as u64;
            min_fee = min_fee.min(entry.fee_bps);
            max_fee = max_fee.max(entry.fee_bps);
            
            if entry.adjustment_reason == AdjustmentReason::EmergencyOverride {
                emergency_count += 1;
            }
        }

        let avg_fee = (total_fee / history.len() as u64) as u32;

        // Calculate fee volatility (simplified standard deviation)
        let mut variance_sum: u64 = 0;
        for entry in history.iter() {
            let diff = if entry.fee_bps > avg_fee {
                (entry.fee_bps - avg_fee) as u64
            } else {
                (avg_fee - entry.fee_bps) as u64
            };
            variance_sum += diff * diff;
        }
        let variance = variance_sum / history.len() as u64;
        let volatility = (Self::integer_sqrt(variance)) as u32;

        FeeHistoryStats {
            avg_fee_bps: avg_fee,
            min_fee_bps: min_fee,
            max_fee_bps: max_fee,
            adjustment_count: history.len() as u32,
            period_days: (period_seconds / 86400) as u32,
            fee_volatility: volatility,
            emergency_override_count: emergency_count,
            period_seconds,
        }
    }

    /// Store user fee snapshot for transparency
    pub fn record_user_fee(env: &Env, user: Address, snapshot: FeeSnapshot) {
        let mut user_fees: Map<Address, FeeSnapshot> = env
            .storage()
            .persistent()
            .get(&Self::USER_FEES_KEY)
            .unwrap_or_else(|| Map::new(env));

        user_fees.set(user, snapshot);
        env.storage()
            .persistent()
            .set(&Self::USER_FEES_KEY, &user_fees);
    }

    /// Get current fee for specific user
    pub fn get_user_fee(env: &Env, user: Address) -> Option<FeeSnapshot> {
        let user_fees: Map<Address, FeeSnapshot> = env
            .storage()
            .persistent()
            .get(&Self::USER_FEES_KEY)
            .unwrap_or_else(|| Map::new(env));

        user_fees.get(user)
    }

    /// Get all fee changes in order (paginated)
    pub fn get_fee_changes_paginated(
        env: &Env,
        page: u32,
        page_size: u32,
    ) -> (Vec<FeeHistoryEntry>, u32) {
        let history: Vec<FeeHistoryEntry> = env
            .storage()
            .persistent()
            .get(&Self::HISTORY_LOG_KEY)
            .unwrap_or_else(|| Vec::new(env));

        let total_pages = (history.len() as u32 + page_size - 1) / page_size;
        
        if page >= total_pages && total_pages > 0 {
            return (Vec::new(env), total_pages);
        }

        let start_idx = (page * page_size) as usize;
        let end_idx = ((page + 1) * page_size).min(history.len() as u32) as usize;

        let mut result = Vec::new(env);
        for i in start_idx..end_idx {
            result.push_back(history.get(i as u32).unwrap());
        }

        (result, total_pages)
    }

    /// Export fee history as CSV-like format for external analysis
    pub fn get_history_summary(
        env: &Env,
        num_entries: u32,
    ) -> Vec<(u64, u32, Symbol)> {
        let history = Self::get_recent_history(env, num_entries);
        
        let mut result = Vec::new(env);
        for entry in history.iter() {
            result.push_back((
                entry.timestamp,
                entry.fee_bps,
                match entry.adjustment_reason {
                    AdjustmentReason::AutomaticCongestionAdjustment => symbol_short!("auto"),
                    AdjustmentReason::ManualAdminAdjustment => symbol_short!("admin"),
                    AdjustmentReason::EmergencyOverride => symbol_short!("emerg"),
                    AdjustmentReason::EmergencyRecovery => symbol_short!("recov"),
                    AdjustmentReason::ScheduledMaintenance => symbol_short!("maint"),
                    AdjustmentReason::SystemInitialization => symbol_short!("init"),
                },
            ));
        }
        result
    }

    /// Clear old history entries beyond retention period  
    pub fn cleanup_old_history(env: &Env, retention_days: u32, current_time: u64) {
        let retention_seconds = (retention_days as u64) * 86400;
        let cutoff_time = current_time.saturating_sub(retention_seconds);

        let history: Vec<FeeHistoryEntry> = env
            .storage()
            .persistent()
            .get(&Self::HISTORY_LOG_KEY)
            .unwrap_or_else(|| Vec::new(env));

        let mut new_history = Vec::new(env);
        for entry in history.iter() {
            if entry.timestamp >= cutoff_time {
                new_history.push_back(entry);
            }
        }

        env.storage()
            .persistent()
            .set(&Self::HISTORY_LOG_KEY, &new_history);
    }

    /// Integer square root for calculating volatility
    fn integer_sqrt(n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let mut x = n;
        let mut y = (x + 1) >> 1;
        while y < x {
            x = y;
            y = (x + n / x) >> 1;
        }
        x
    }

    /// Get the last recorded fee
    pub fn get_last_fee(env: &Env) -> Option<u32> {
        let history: Vec<FeeHistoryEntry> = env
            .storage()
            .persistent()
            .get(&Self::HISTORY_LOG_KEY)
            .unwrap_or_else(|| Vec::new(env));

        if history.is_empty() {
            None
        } else {
            Some(history.get((history.len() - 1) as u32).unwrap().fee_bps)
        }
    }

    /// Count total fee adjustments due to congestion
    pub fn count_automatic_adjustments(env: &Env, period_seconds: u64, current_time: u64) -> u32 {
        let start_time = current_time.saturating_sub(period_seconds);
        let history = Self::get_history_range(env, start_time, current_time);

        let mut count = 0u32;
        for entry in history.iter() {
            if entry.adjustment_reason == AdjustmentReason::AutomaticCongestionAdjustment {
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjustment_reason_equality() {
        assert_eq!(
            AdjustmentReason::AutomaticCongestionAdjustment,
            AdjustmentReason::AutomaticCongestionAdjustment
        );
        assert_ne!(
            AdjustmentReason::AutomaticCongestionAdjustment,
            AdjustmentReason::EmergencyOverride
        );
    }

    #[test]
    fn test_integer_sqrt() {
        assert_eq!(FeeHistoryManager::integer_sqrt(0), 0);
        assert_eq!(FeeHistoryManager::integer_sqrt(1), 1);
        assert_eq!(FeeHistoryManager::integer_sqrt(4), 2);
        assert_eq!(FeeHistoryManager::integer_sqrt(16), 4);
        assert_eq!(FeeHistoryManager::integer_sqrt(100), 10);
        assert_eq!(FeeHistoryManager::integer_sqrt(144), 12);
    }

    #[test]
    fn test_fee_adjustment_entry_creation() {
        let metrics = NetworkMetrics {
            txn_volume_tps: 100,
            avg_gas_price: 100,
            pending_txn_count: 100,
            ledger_close_time: 1000,
            capacity_utilization_percent: 10,
            timestamp: 1000,
            avg_confirmation_time_ms: 2000,
        };

        // This test verifies the structure is valid
        let _entry = FeeHistoryEntry {
            timestamp: 1000,
            fee_bps: 50,
            previous_fee_bps: 45,
            congestion_level: CongestionLevel::VeryLow,
            network_metrics: metrics,
            adjustment_reason: AdjustmentReason::AutomaticCongestionAdjustment,
            triggered_by: symbol_short!("auto"),
        };
    }
}
