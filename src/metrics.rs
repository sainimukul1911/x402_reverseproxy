//! Prometheus metrics module for observability.
//!
//! Exports metrics for monitoring with Grafana/Prometheus.

use once_cell::sync::Lazy;
use prometheus::{
    register_counter_vec, register_gauge, register_histogram_vec, CounterVec, Encoder, Gauge,
    HistogramVec, TextEncoder,
};

/// Total requests counter (by status: allowed, rate_limited, paid)
pub static REQUESTS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "x402_proxy_requests_total",
        "Total number of requests processed",
        &["status", "method"]
    )
    .expect("Failed to register REQUESTS_TOTAL metric")
});

/// Request duration histogram
pub static REQUEST_DURATION: Lazy<HistogramVec> = Lazy::new(|| {
    register_histogram_vec!(
        "x402_proxy_request_duration_seconds",
        "Request duration in seconds",
        &["status"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    )
    .expect("Failed to register REQUEST_DURATION metric")
});

/// Rate limit blocked requests counter
pub static RATE_LIMITED_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "x402_proxy_rate_limited_total",
        "Total number of rate-limited requests",
        &["layer"] // "ip" or "global"
    )
    .expect("Failed to register RATE_LIMITED_TOTAL metric")
});

/// x402 payments counter
pub static PAYMENTS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "x402_proxy_payments_total",
        "Total number of x402 payment attempts",
        &["status"] // "verified", "settled", "failed"
    )
    .expect("Failed to register PAYMENTS_TOTAL metric")
});

/// Currently tracked IPs gauge
pub static TRACKED_IPS: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "x402_proxy_tracked_ips",
        "Current number of IPs being tracked by rate limiter"
    )
    .expect("Failed to register TRACKED_IPS metric")
});

/// Active connections gauge
pub static ACTIVE_CONNECTIONS: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "x402_proxy_active_connections",
        "Current number of active connections"
    )
    .expect("Failed to register ACTIVE_CONNECTIONS metric")
});

/// Initialize all metrics (forces lazy initialization)
pub fn init_metrics() {
    // Force initialization of all lazy statics
    Lazy::force(&REQUESTS_TOTAL);
    Lazy::force(&REQUEST_DURATION);
    Lazy::force(&RATE_LIMITED_TOTAL);
    Lazy::force(&PAYMENTS_TOTAL);
    Lazy::force(&TRACKED_IPS);
    Lazy::force(&ACTIVE_CONNECTIONS);
}

/// Generate Prometheus metrics output
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap_or_default()
}

/// Record a request with the given status
pub fn record_request(status: &str, method: &str) {
    REQUESTS_TOTAL.with_label_values(&[status, method]).inc();
}

/// Record a rate-limited request
pub fn record_rate_limited(layer: &str) {
    RATE_LIMITED_TOTAL.with_label_values(&[layer]).inc();
}

/// Record a payment attempt
pub fn record_payment(status: &str) {
    PAYMENTS_TOTAL.with_label_values(&[status]).inc();
}

/// Update tracked IPs count
pub fn set_tracked_ips(count: usize) {
    TRACKED_IPS.set(count as f64);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_init() {
        init_metrics();
        let output = gather_metrics();
        assert!(output.contains("x402_proxy"));
    }
}
