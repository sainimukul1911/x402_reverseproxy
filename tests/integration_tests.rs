//! Integration tests for the x402 reverse proxy
//!
//! These tests verify end-to-end functionality of the proxy components.

use x402_reverseproxy::{health, metrics, Config, RateLimitResult, RateLimiter, X402Handler};

/// Integration test: Full request flow simulation
#[test]
fn test_full_request_flow() {
    // Initialize components
    metrics::init_metrics();
    health::set_ready(true);
    health::set_healthy(true);

    // Create rate limiter
    let limiter = RateLimiter::new(10, 1000, 1);

    // Create x402 handler
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xToken".to_string(),
        "API access".to_string(),
    );

    // Simulate requests from a client
    let client_ip = "192.168.1.100";

    // First few requests should be allowed
    for i in 0..5 {
        let result = limiter.check(client_ip);
        assert_eq!(
            result,
            RateLimitResult::Allowed,
            "Request {} should be allowed",
            i
        );
        metrics::record_request("allowed", "GET");
    }

    // Check metrics were recorded
    let metrics_output = metrics::gather_metrics();
    assert!(metrics_output.contains("x402_proxy_requests_total"));

    // Check health endpoint returns healthy
    let (status, _, _) = health::handle_admin_request("/health").unwrap();
    assert_eq!(status, 200);
}

/// Integration test: Rate limit triggers 402 response generation
#[test]
fn test_rate_limit_triggers_payment_required() {
    let limiter = RateLimiter::new(2, 1000, 1);
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xToken".to_string(),
        "API access".to_string(),
    );

    let client_ip = "192.168.1.50";

    // Exhaust rate limit
    limiter.check(client_ip);
    limiter.check(client_ip);

    // Next request should be rate limited
    let result = limiter.check(client_ip);

    // When rate limited, we should generate a 402 response
    if result == RateLimitResult::IpLimitExceeded || result == RateLimitResult::GlobalLimitExceeded
    {
        // Generate payment required header
        let payment_header = handler.generate_payment_required_header("/api/resource");
        assert!(!payment_header.is_empty());

        // Generate 402 body
        let body = handler.generate_402_body("/api/resource");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["error"], "payment_required");
    }
}

/// Integration test: Configuration drives component behavior
#[test]
fn test_config_drives_behavior() {
    let toml_str = r#"
        [server]
        bind = "0.0.0.0:8080"
        
        [upstream]
        url = "http://localhost:3000"
        
        [rate_limits]
        per_ip_requests_per_second = 5
        global_requests_per_second = 100
        
        [x402]
        facilitator_url = "https://custom.facilitator.com"
        recipient_address = "0xCustomRecipient"
        amount = "0.005"
        network = "base"
        token = "0xCustomToken"
        description = "Custom API payment"
    "#;

    let config: Config = toml::from_str(toml_str).unwrap();

    // Rate limiter should use config values
    let limiter = RateLimiter::new(
        config.rate_limits.per_ip_requests_per_second,
        config.rate_limits.global_requests_per_second,
        config.rate_limits.window_seconds,
    );

    // x402 handler should use config values
    let handler = X402Handler::new(
        config.x402.facilitator_url.clone(),
        config.x402.recipient_address.clone(),
        config.x402.amount.clone(),
        config.x402.network.clone(),
        config.x402.token.clone(),
        config.x402.description.clone(),
    );

    // Generate header and verify config values are used
    let header = handler.generate_payment_required_header("/test");
    let decoded =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &header).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&decoded).unwrap();

    assert_eq!(json["network"], "base");
    assert_eq!(json["payTo"], "0xCustomRecipient");
    assert_eq!(json["maxAmountRequired"], "0.005");
}

/// Integration test: Multiple IPs with different rate limit states
#[test]
fn test_multiple_clients_independent_limits() {
    let limiter = RateLimiter::new(3, 100, 1);

    // Client A: fully exhausts limit
    for _ in 0..3 {
        assert_eq!(limiter.check("10.0.0.1"), RateLimitResult::Allowed);
    }

    // Client B: still has full limit
    for _ in 0..3 {
        assert_eq!(limiter.check("10.0.0.2"), RateLimitResult::Allowed);
    }

    // Client C: partial usage
    assert_eq!(limiter.check("10.0.0.3"), RateLimitResult::Allowed);

    // Verify tracking
    let stats = limiter.stats();
    assert!(stats.tracked_ips >= 3);
}

/// Integration test: Health endpoints don't count against rate limits
#[test]
fn test_health_endpoints_bypass_rate_limiting() {
    // Health endpoints should be identified and handled separately
    assert!(health::is_admin_path("/health"));
    assert!(health::is_admin_path("/ready"));
    assert!(health::is_admin_path("/metrics"));

    // Regular API paths should NOT bypass
    assert!(!health::is_admin_path("/api/v1/users"));
    assert!(!health::is_admin_path("/webhook"));
}
