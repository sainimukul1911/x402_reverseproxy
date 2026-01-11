//! Rate limiting module implementing dual-layer defense.
//!
//! - Layer 1: Per-IP rate limiting - blocks individual abusive IPs
//! - Layer 2: Global rate limiting - DDoS protection for the entire service

use parking_lot::RwLock;
use pingora_limits::rate::Rate;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Result of a rate limit check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    /// Request is allowed (within limits)
    Allowed,
    /// Per-IP limit exceeded (Layer 1)
    IpLimitExceeded,
    /// Global limit exceeded (Layer 2)
    GlobalLimitExceeded,
}

/// Dual-layer rate limiter for API protection.
pub struct RateLimiter {
    /// Per-IP rate limiters (Layer 1)
    ip_limiters: Arc<RwLock<HashMap<String, Rate>>>,
    /// Global rate limiter (Layer 2)  
    global_limiter: Arc<Rate>,
    /// Per-IP requests per second limit
    per_ip_limit: Arc<AtomicU64>,
    /// Global requests per second limit
    global_limit: Arc<AtomicU64>,
    /// Time window for rate calculations
    window: Duration,
}

impl RateLimiter {
    /// Create a new rate limiter with the specified limits.
    pub fn new(per_ip_rps: u32, global_rps: u32, window_seconds: u64) -> Self {
        let window = Duration::from_secs(window_seconds);

        Self {
            ip_limiters: Arc::new(RwLock::new(HashMap::new())),
            global_limiter: Arc::new(Rate::new(window)),
            per_ip_limit: Arc::new(AtomicU64::new(per_ip_rps as u64)),
            global_limit: Arc::new(AtomicU64::new(global_rps as u64)),
            window,
        }
    }

    /// Update rate limits dynamically.
    pub fn update_limits(&self, per_ip_rps: u32, global_rps: u32) {
        self.per_ip_limit
            .store(per_ip_rps as u64, Ordering::Relaxed);
        self.global_limit
            .store(global_rps as u64, Ordering::Relaxed);
    }

    /// Check if a request from the given IP should be allowed.
    ///
    /// Returns the rate limit result indicating whether the request
    /// is allowed or which limit was exceeded.
    pub fn check(&self, client_ip: &str) -> RateLimitResult {
        // Layer 2: Check global rate limit first (DDoS protection)
        // Observe the event first, then check the rate
        self.global_limiter.observe(&"global", 1);
        let global_rate = self.global_limiter.rate(&"global");
        let global_limit = self.global_limit.load(Ordering::Relaxed) as f64;

        if global_rate > global_limit {
            return RateLimitResult::GlobalLimitExceeded;
        }

        // Layer 1: Check per-IP rate limit
        {
            let mut ip_limiters = self.ip_limiters.write();
            let limiter = ip_limiters
                .entry(client_ip.to_string())
                .or_insert_with(|| Rate::new(self.window));

            limiter.observe(&client_ip, 1);
            let ip_rate = limiter.rate(&client_ip);
            let per_ip_limit = self.per_ip_limit.load(Ordering::Relaxed) as f64;

            if ip_rate > per_ip_limit {
                return RateLimitResult::IpLimitExceeded;
            }
        }

        RateLimitResult::Allowed
    }

    /// Clean up old IP entries to prevent memory growth.
    /// Should be called periodically (e.g., every minute).
    pub fn cleanup_stale_entries(&self) {
        let mut ip_limiters = self.ip_limiters.write();
        // Remove entries with zero rate (haven't been accessed recently)
        ip_limiters.retain(|ip, limiter| limiter.rate(ip) > 0.0);
    }

    /// Get current stats for monitoring.
    pub fn stats(&self) -> RateLimiterStats {
        let ip_limiters = self.ip_limiters.read();

        RateLimiterStats {
            tracked_ips: ip_limiters.len(),
            global_rate: self.global_limiter.rate(&"global"),
        }
    }
}

/// Statistics about the rate limiter state.
#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    /// Number of IPs currently being tracked
    pub tracked_ips: usize,
    /// Current global request rate
    pub global_rate: f64,
}

impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            ip_limiters: Arc::clone(&self.ip_limiters),
            global_limiter: Arc::clone(&self.global_limiter),
            per_ip_limit: Arc::clone(&self.per_ip_limit),
            global_limit: Arc::clone(&self.global_limit),
            window: self.window,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_within_limit() {
        let limiter = RateLimiter::new(10, 100, 1);

        // First request should be allowed
        assert_eq!(limiter.check("192.168.1.1"), RateLimitResult::Allowed);
    }

    #[test]
    fn test_stats() {
        let limiter = RateLimiter::new(10, 100, 1);

        // Make a request to track an IP
        limiter.check("192.168.1.1");

        let stats = limiter.stats();
        assert!(stats.tracked_ips >= 1);
        // Note: global_rate may be 0.0 depending on timing/estimation
        // Just verify it doesn't panic
        let _ = stats.global_rate;
    }
}

/// Background service to clean up stale rate limiter entries.
pub struct RateLimitCleaner(pub RateLimiter);

#[async_trait::async_trait]
impl pingora_core::services::Service for RateLimitCleaner {
    async fn start_service(
        &mut self,
        _shutdown: tokio::sync::watch::Receiver<bool>,
        _threads: usize,
    ) {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            self.0.cleanup_stale_entries();
        }
    }

    fn name(&self) -> &str {
        "RateLimitCleaner"
    }
}
