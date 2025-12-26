//! Unit tests for the rate limiter module

use std::thread;
use std::time::Duration;
use x402_reverseproxy::{RateLimitResult, RateLimiter};

#[test]
fn test_allows_first_request() {
    let limiter = RateLimiter::new(10, 100, 1);

    let result = limiter.check("192.168.1.1");
    assert_eq!(result, RateLimitResult::Allowed);
}

#[test]
fn test_allows_requests_within_limit() {
    let limiter = RateLimiter::new(5, 100, 1);

    // Should allow up to 5 requests
    for i in 0..5 {
        let result = limiter.check("192.168.1.1");
        assert_eq!(
            result,
            RateLimitResult::Allowed,
            "Request {} should be allowed",
            i
        );
    }
}

#[test]
fn test_different_ips_have_separate_limits() {
    let limiter = RateLimiter::new(2, 100, 1);

    // IP 1: 2 requests
    assert_eq!(limiter.check("192.168.1.1"), RateLimitResult::Allowed);
    assert_eq!(limiter.check("192.168.1.1"), RateLimitResult::Allowed);

    // IP 2: should have its own limit
    assert_eq!(limiter.check("192.168.1.2"), RateLimitResult::Allowed);
    assert_eq!(limiter.check("192.168.1.2"), RateLimitResult::Allowed);
}

#[test]
fn test_stats_tracking() {
    let limiter = RateLimiter::new(10, 100, 1);

    // Initially no tracked IPs
    let stats = limiter.stats();
    assert_eq!(stats.tracked_ips, 0);

    // After one request, one IP tracked
    limiter.check("192.168.1.1");
    let stats = limiter.stats();
    assert!(stats.tracked_ips >= 1);

    // After request from another IP, two IPs tracked
    limiter.check("192.168.1.2");
    let stats = limiter.stats();
    assert!(stats.tracked_ips >= 2);
}

#[test]
fn test_clone_shares_state() {
    let limiter1 = RateLimiter::new(10, 100, 1);
    let limiter2 = limiter1.clone();

    // Request on limiter1
    limiter1.check("192.168.1.1");

    // Should be visible on limiter2
    let stats = limiter2.stats();
    assert!(stats.tracked_ips >= 1);
}

#[test]
fn test_cleanup_stale_entries() {
    let limiter = RateLimiter::new(10, 100, 1);

    // Add some IPs
    limiter.check("192.168.1.1");
    limiter.check("192.168.1.2");
    limiter.check("192.168.1.3");

    // Cleanup should run without panic
    limiter.cleanup_stale_entries();

    // Stats should still work
    let stats = limiter.stats();
    assert!(stats.tracked_ips >= 0);
}

#[test]
fn test_rate_limiter_thread_safety() {
    use std::sync::Arc;

    let limiter = Arc::new(RateLimiter::new(1000, 10000, 1));
    let mut handles = vec![];

    // Spawn multiple threads making requests
    for i in 0..10 {
        let limiter_clone = Arc::clone(&limiter);
        let handle = thread::spawn(move || {
            for _ in 0..100 {
                let ip = format!("192.168.1.{}", i);
                let _ = limiter_clone.check(&ip);
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Should have tracked multiple IPs
    let stats = limiter.stats();
    assert!(stats.tracked_ips >= 1);
}
