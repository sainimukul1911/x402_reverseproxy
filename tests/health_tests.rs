//! Unit tests for the health check module

use x402_reverseproxy::health;

#[test]
fn test_is_admin_path() {
    // Should match health endpoints
    assert!(health::is_admin_path("/health"));
    assert!(health::is_admin_path("/ready"));
    assert!(health::is_admin_path("/metrics"));
    assert!(health::is_admin_path("/_health"));
    assert!(health::is_admin_path("/_ready"));
    assert!(health::is_admin_path("/_metrics"));

    // Should NOT match regular paths
    assert!(!health::is_admin_path("/api/users"));
    assert!(!health::is_admin_path("/"));
    assert!(!health::is_admin_path("/health/detailed"));
    assert!(!health::is_admin_path("/v1/health"));
}

#[test]
fn test_health_endpoint_healthy() {
    health::set_healthy(true);

    let result = health::handle_admin_request("/health");
    assert!(result.is_some());

    let (status, body, content_type) = result.unwrap();
    assert_eq!(status, 200);
    assert_eq!(content_type, "application/json");

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "healthy");
}

#[test]
fn test_health_endpoint_unhealthy() {
    health::set_healthy(false);

    let result = health::handle_admin_request("/health");
    assert!(result.is_some());

    let (status, body, _) = result.unwrap();
    assert_eq!(status, 503);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["status"], "unhealthy");

    // Reset for other tests
    health::set_healthy(true);
}

#[test]
fn test_ready_endpoint() {
    health::set_ready(true);

    let result = health::handle_admin_request("/ready");
    assert!(result.is_some());

    let (status, body, _) = result.unwrap();
    assert_eq!(status, 200);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["ready"], true);
}

#[test]
fn test_ready_endpoint_not_ready() {
    health::set_ready(false);

    let result = health::handle_admin_request("/ready");
    assert!(result.is_some());

    let (status, body, _) = result.unwrap();
    assert_eq!(status, 503);

    let json: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(json["ready"], false);
}

#[test]
fn test_metrics_endpoint() {
    // Initialize metrics first
    x402_reverseproxy::metrics::init_metrics();

    let result = health::handle_admin_request("/metrics");
    assert!(result.is_some());

    let (status, body, content_type) = result.unwrap();
    assert_eq!(status, 200);
    assert!(content_type.contains("text/plain"));

    // Should contain prometheus-style metrics
    assert!(body.contains("x402_proxy"));
}

#[test]
fn test_unknown_path_returns_none() {
    let result = health::handle_admin_request("/api/users");
    assert!(result.is_none());

    let result = health::handle_admin_request("/");
    assert!(result.is_none());
}

#[test]
fn test_health_response_contains_version() {
    health::set_healthy(true);

    let (_, body, _) = health::handle_admin_request("/health").unwrap();
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert!(json["version"].as_str().is_some());
}

#[test]
fn test_is_healthy_and_is_ready() {
    // Test the accessor functions
    health::set_healthy(true);
    assert!(health::is_healthy());

    health::set_healthy(false);
    assert!(!health::is_healthy());

    health::set_ready(true);
    assert!(health::is_ready());

    health::set_ready(false);
    assert!(!health::is_ready());

    // Reset
    health::set_healthy(true);
}
