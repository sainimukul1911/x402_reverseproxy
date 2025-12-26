//! Unit tests for the metrics module

use x402_reverseproxy::metrics;

#[test]
fn test_metrics_initialization() {
    // Should not panic
    metrics::init_metrics();
}

#[test]
fn test_gather_metrics() {
    metrics::init_metrics();
    
    let output = metrics::gather_metrics();
    
    // Should contain our custom metrics
    assert!(output.contains("x402_proxy_requests_total"));
    assert!(output.contains("x402_proxy_rate_limited_total"));
    assert!(output.contains("x402_proxy_payments_total"));
    assert!(output.contains("x402_proxy_tracked_ips"));
}

#[test]
fn test_record_request() {
    metrics::init_metrics();
    
    // Record some requests
    metrics::record_request("allowed", "GET");
    metrics::record_request("allowed", "POST");
    metrics::record_request("rate_limited", "GET");
    metrics::record_request("paid", "GET");
    
    let output = metrics::gather_metrics();
    assert!(output.contains("x402_proxy_requests_total"));
}

#[test]
fn test_record_rate_limited() {
    metrics::init_metrics();
    
    metrics::record_rate_limited("ip");
    metrics::record_rate_limited("global");
    
    let output = metrics::gather_metrics();
    assert!(output.contains("x402_proxy_rate_limited_total"));
}

#[test]
fn test_record_payment() {
    metrics::init_metrics();
    
    metrics::record_payment("verified");
    metrics::record_payment("settled");
    metrics::record_payment("failed");
    
    let output = metrics::gather_metrics();
    assert!(output.contains("x402_proxy_payments_total"));
}

#[test]
fn test_set_tracked_ips() {
    metrics::init_metrics();
    
    metrics::set_tracked_ips(10);
    metrics::set_tracked_ips(25);
    
    let output = metrics::gather_metrics();
    assert!(output.contains("x402_proxy_tracked_ips"));
}

#[test]
fn test_metrics_thread_safety() {
    use std::thread;
    use std::sync::Arc;
    
    metrics::init_metrics();
    
    let mut handles = vec![];
    
    for _ in 0..10 {
        let handle = thread::spawn(|| {
            for _ in 0..100 {
                metrics::record_request("allowed", "GET");
                metrics::record_rate_limited("ip");
            }
        });
        handles.push(handle);
    }
    
    for handle in handles {
        handle.join().unwrap();
    }
    
    // Should gather without issues
    let output = metrics::gather_metrics();
    assert!(!output.is_empty());
}
