use crate::error::{MarketDataError, Result};
use governor::{Quota, RateLimiter as GovernorRateLimiter, Jitter, clock::DefaultClock, state::InMemoryState};
use std::num::NonZeroU32;
use std::sync::Arc;
use dashmap::DashMap;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use tracing::{warn, info, debug};

pub struct RateLimiter {
    connection_limiters: Arc<DashMap<Uuid, Arc<ConnectionRateLimiter>>>,
    global_limiter: Arc<GlobalRateLimiter>,
    config: RateLimitConfig,
    stats: Arc<RwLock<RateLimitStats>>,
}

pub struct ConnectionRateLimiter {
    pub connection_id: Uuid,
    message_limiter: GovernorRateLimiter<InMemoryState, DefaultClock>,
    bandwidth_limiter: GovernorRateLimiter<InMemoryState, DefaultClock>,
    subscription_limiter: GovernorRateLimiter<InMemoryState, DefaultClock>,
    stats: ConnectionRateLimitStats,
    created_at: DateTime<Utc>,
}

pub struct GlobalRateLimiter {
    total_connections_limiter: GovernorRateLimiter<InMemoryState, DefaultClock>,
    total_messages_limiter: GovernorRateLimiter<InMemoryState, DefaultClock>,
    total_bandwidth_limiter: GovernorRateLimiter<InMemoryState, DefaultClock>,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub max_connections_per_second: u32,
    pub max_messages_per_connection_per_second: u32,
    pub max_bandwidth_per_connection_mb_per_second: u32,
    pub max_subscriptions_per_connection: u32,
    pub max_total_messages_per_second: u32,
    pub max_total_bandwidth_mb_per_second: u32,
    pub burst_size_multiplier: f32,
    pub penalty_duration_seconds: u64,
    pub whitelist_ips: Vec<String>,
}

#[derive(Debug, Default, Clone)]
pub struct RateLimitStats {
    pub total_requests: u64,
    pub allowed_requests: u64,
    pub blocked_requests: u64,
    pub penalized_connections: u64,
    pub average_request_rate: f64,
    pub peak_request_rate: f64,
}

#[derive(Debug, Default, Clone)]
pub struct ConnectionRateLimitStats {
    pub requests: u64,
    pub allowed: u64,
    pub blocked: u64,
    pub penalized: bool,
    pub penalty_until: Option<DateTime<Utc>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        let config = RateLimitConfig::default();
        Self::with_config(config)
    }

    pub fn with_config(config: RateLimitConfig) -> Self {
        Self {
            connection_limiters: Arc::new(DashMap::new()),
            global_limiter: Arc::new(GlobalRateLimiter::new(&config)),
            config,
            stats: Arc::new(RwLock::new(RateLimitStats::default())),
        }
    }

    pub fn add_connection(&self, connection_id: Uuid) -> Result<()> {
        // Check global connection limit
        if !self.global_limiter.can_add_connection() {
            warn!("Global connection limit reached, rejecting connection: {}", connection_id);
            return Err(MarketDataError::RateLimitExceeded);
        }

        let connection_limiter = Arc::new(ConnectionRateLimiter::new(connection_id, &self.config));
        self.connection_limiters.insert(connection_id, connection_limiter);
        
        info!("Added rate limiter for connection: {}", connection_id);
        Ok(())
    }

    pub fn remove_connection(&self, connection_id: Uuid) {
        self.connection_limiters.remove(&connection_id);
        info!("Removed rate limiter for connection: {}", connection_id);
    }

    pub fn check_limit(&self, connection_id: Uuid) -> bool {
        let start_time = std::time::Instant::now();
        let mut result = true;

        // Update global stats
        {
            let mut stats = self.stats.write();
            stats.total_requests += 1;
        }

        // Check if connection is penalized
        if let Some(limiter) = self.connection_limiters.get(&connection_id) {
            if limiter.is_penalized() {
                warn!("Connection {} is penalized, blocking request", connection_id);
                result = false;
            } else {
                result = limiter.check_message_limit() && 
                        limiter.check_bandwidth_limit(1024) && // Assume 1KB message
                        self.global_limiter.check_message_limit() &&
                        self.global_limiter.check_bandwidth_limit(1024);
            }
        } else {
            // Connection not found, create new limiter
            if let Err(_) = self.add_connection(connection_id) {
                result = false;
            }
        }

        // Update statistics
        let check_time = start_time.elapsed();
        {
            let mut stats = self.stats.write();
            if result {
                stats.allowed_requests += 1;
            } else {
                stats.blocked_requests += 1;
            }
            
            // Update average request rate (simplified)
            let total_time = stats.average_request_rate * (stats.total_requests - 1) as f64;
            stats.average_request_rate = (total_time + check_time.as_nanos() as f64) / stats.total_requests as f64;
        }

        debug!("Rate limit check for connection {}: {} (took {:?})", 
               connection_id, result, check_time);

        result
    }

    pub fn check_subscription_limit(&self, connection_id: Uuid, current_subscriptions: usize) -> bool {
        if current_subscriptions >= self.config.max_subscriptions_per_connection as usize {
            warn!("Connection {} exceeded subscription limit", connection_id);
            return false;
        }

        if let Some(limiter) = self.connection_limiters.get(&connection_id) {
            limiter.check_subscription_limit()
        } else {
            false
        }
    }

    pub fn penalize_connection(&self, connection_id: Uuid, reason: &str) {
        if let Some(limiter) = self.connection_limiters.get(&connection_id) {
            limiter.penalize(&self.config, reason);
            
            // Update global stats
            let mut stats = self.stats.write();
            stats.penalized_connections += 1;
        }
    }

    pub fn is_whitelisted(&self, ip: &str) -> bool {
        self.config.whitelist_ips.contains(&ip.to_string())
    }

    pub fn get_connection_stats(&self, connection_id: Uuid) -> Option<ConnectionRateLimitStats> {
        self.connection_limiters.get(&connection_id)
            .map(|limiter| limiter.get_stats())
    }

    pub fn get_global_stats(&self) -> RateLimitStats {
        self.stats.read().clone()
    }

    pub fn cleanup_expired_penalties(&self) -> usize {
        let mut cleaned_count = 0;
        let now = Utc::now();

        for entry in self.connection_limiters.iter() {
            let limiter = entry.value();
            if limiter.is_penalty_expired(now) {
                limiter.clear_penalty();
                cleaned_count += 1;
            }
        }

        if cleaned_count > 0 {
            info!("Cleared {} expired penalties", cleaned_count);
        }

        cleaned_count
    }

    pub fn update_config(&mut self, new_config: RateLimitConfig) {
        self.config = new_config;
        info!("Updated rate limit configuration");
    }
}

impl ConnectionRateLimiter {
    pub fn new(connection_id: Uuid, config: &RateLimitConfig) -> Self {
        let burst_size = (config.max_messages_per_connection_per_second as f32 * config.burst_size_multiplier) as u32;
        let message_quota = Quota::per_second(NonZeroU32::new(config.max_messages_per_connection_per_second).unwrap())
            .allow_burst(NonZeroU32::new(burst_size).unwrap());
        
        let bandwidth_burst = (config.max_bandwidth_per_connection_mb_per_second as f32 * config.burst_size_multiplier) as u32;
        let bandwidth_quota = Quota::per_second(NonZeroU32::new(config.max_bandwidth_per_connection_mb_per_second * 1024 * 1024 / 1024).unwrap())
            .allow_burst(NonZeroU32::new(bandwidth_burst).unwrap());

        let subscription_quota = Quota::per_second(NonZeroU32::new(10).unwrap())
            .allow_burst(NonZeroU32::new(20).unwrap());

        Self {
            connection_id,
            message_limiter: GovernorRateLimiter::direct(message_quota),
            bandwidth_limiter: GovernorRateLimiter::direct(bandwidth_quota),
            subscription_limiter: GovernorRateLimiter::direct(subscription_quota),
            stats: ConnectionRateLimitStats::default(),
            created_at: Utc::now(),
        }
    }

    pub fn check_message_limit(&self) -> bool {
        let allowed = self.message_limiter.check().is_ok();
        
        // Update stats (note: this would need interior mutability in a real implementation)
        // For now, we'll just return the result
        allowed
    }

    pub fn check_bandwidth_limit(&self, bytes: u32) -> bool {
        let allowed = self.bandwidth_limiter.check().is_ok();
        allowed
    }

    pub fn check_subscription_limit(&self) -> bool {
        let allowed = self.subscription_limiter.check().is_ok();
        allowed
    }

    pub fn penalize(&self, config: &RateLimitConfig, reason: &str) {
        warn!("Penalizing connection {} for: {}", self.connection_id, reason);
        // In a real implementation, this would set penalty_until
    }

    pub fn is_penalized(&self) -> bool {
        // Check if current time is before penalty_until
        false // Simplified
    }

    pub fn is_penalty_expired(&self, now: DateTime<Utc>) -> bool {
        // Check if penalty has expired
        true // Simplified
    }

    pub fn clear_penalty(&self) {
        // Clear the penalty
    }

    pub fn get_stats(&self) -> ConnectionRateLimitStats {
        self.stats.clone()
    }
}

impl GlobalRateLimiter {
    pub fn new(config: &RateLimitConfig) -> Self {
        let connection_quota = Quota::per_second(NonZeroU32::new(config.max_connections_per_second).unwrap());
        let message_quota = Quota::per_second(NonZeroU32::new(config.max_total_messages_per_second).unwrap());
        let bandwidth_quota = Quota::per_second(NonZeroU32::new(config.max_total_bandwidth_mb_per_second * 1024 * 1024 / 1024).unwrap());

        Self {
            total_connections_limiter: GovernorRateLimiter::direct(connection_quota),
            total_messages_limiter: GovernorRateLimiter::direct(message_quota),
            total_bandwidth_limiter: GovernorRateLimiter::direct(bandwidth_quota),
        }
    }

    pub fn can_add_connection(&self) -> bool {
        self.total_connections_limiter.check().is_ok()
    }

    pub fn check_message_limit(&self) -> bool {
        self.total_messages_limiter.check().is_ok()
    }

    pub fn check_bandwidth_limit(&self, bytes: u32) -> bool {
        self.total_bandwidth_limiter.check().is_ok()
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_connections_per_second: 100,
            max_messages_per_connection_per_second: 100,
            max_bandwidth_per_connection_mb_per_second: 10, // 10 MB/s
            max_subscriptions_per_connection: 50,
            max_total_messages_per_second: 10000,
            max_total_bandwidth_mb_per_second: 1000, // 1 GB/s
            burst_size_multiplier: 2.0,
            penalty_duration_seconds: 300, // 5 minutes
            whitelist_ips: Vec::new(),
        }
    }
}

pub struct DDoSProtection {
    rate_limiter: Arc<RateLimiter>,
    ip_tracker: Arc<DashMap<String, IpStats>>,
    detection_threshold: u32,
    block_duration: std::time::Duration,
}

#[derive(Debug, Default, Clone)]
pub struct IpStats {
    pub request_count: u32,
    pub last_request: DateTime<Utc>,
    pub is_blocked: bool,
    pub blocked_until: Option<DateTime<Utc>>,
    pub suspicious_activity_score: u32,
}

impl DDoSProtection {
    pub fn new(rate_limiter: Arc<RateLimiter>) -> Self {
        Self {
            rate_limiter,
            ip_tracker: Arc::new(DashMap::new()),
            detection_threshold: 1000, // requests per minute
            block_duration: std::time::Duration::from_secs(3600), // 1 hour
        }
    }

    pub fn check_ip(&self, ip: &str) -> bool {
        let now = Utc::now();
        let mut ip_stats = self.ip_tracker.entry(ip.to_string()).or_insert_with(|| IpStats {
            last_request: now,
            ..Default::default()
        });

        // Check if IP is currently blocked
        if ip_stats.is_blocked {
            if let Some(blocked_until) = ip_stats.blocked_until {
                if now < blocked_until {
                    warn!("Blocked IP attempted connection: {}", ip);
                    return false;
                } else {
                    // Block expired
                    ip_stats.is_blocked = false;
                    ip_stats.blocked_until = None;
                }
            }
        }

        // Update request count (simplified - would use sliding window)
        ip_stats.request_count += 1;
        ip_stats.last_request = now;

        // Check for DDoS patterns
        if ip_stats.request_count > self.detection_threshold {
            warn!("DDoS detected from IP: {} (requests: {})", ip, ip_stats.request_count);
            ip_stats.is_blocked = true;
            ip_stats.blocked_until = Some(now + chrono::Duration::from_std(self.block_duration).unwrap());
            return false;
        }

        true
    }

    pub fn report_suspicious_activity(&self, ip: &str, activity_type: &str) {
        if let Some(mut ip_stats) = self.ip_tracker.get_mut(ip) {
            ip_stats.suspicious_activity_score += 1;
            warn!("Suspicious activity from IP {}: {} (score: {})", 
                  ip, activity_type, ip_stats.suspicious_activity_score);
            
            // Auto-block if score is too high
            if ip_stats.suspicious_activity_score > 10 {
                ip_stats.is_blocked = true;
                ip_stats.blocked_until = Some(Utc::now() + chrono::Duration::from_std(self.block_duration).unwrap());
                warn!("Auto-blocked IP due to suspicious activity: {}", ip);
            }
        }
    }

    pub fn get_ip_stats(&self, ip: &str) -> Option<IpStats> {
        self.ip_tracker.get(ip).map(|stats| stats.clone())
    }

    pub fn cleanup_old_entries(&self) {
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        self.ip_tracker.retain(|_, stats| stats.last_request > cutoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_creation() {
        let rate_limiter = RateLimiter::new();
        assert_eq!(rate_limiter.get_global_stats().total_requests, 0);
    }

    #[test]
    fn test_connection_addition() {
        let rate_limiter = RateLimiter::new();
        let connection_id = Uuid::new_v4();
        assert!(rate_limiter.add_connection(connection_id).is_ok());
    }

    #[test]
    fn test_ddos_protection() {
        let rate_limiter = Arc::new(RateLimiter::new());
        let ddos_protection = DDoSProtection::new(rate_limiter);
        
        let ip = "192.168.1.1";
        assert!(ddos_protection.check_ip(ip));
        
        // Report suspicious activity
        for _ in 0..15 {
            ddos_protection.report_suspicious_activity(ip, "test");
        }
        
        // Should be blocked now
        assert!(!ddos_protection.check_ip(ip));
    }
}
