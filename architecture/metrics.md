# metrics.rs - Prometheus Metrics

Exports metrics for monitoring with Prometheus/Grafana.

---

## Overview

This module:
1. Defines all proxy metrics
2. Uses lazy initialization for thread-safe global metrics
3. Provides helper functions to record events
4. Generates Prometheus text format output

---

## Metrics Defined

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `x402_proxy_requests_total` | Counter | status, method | Total requests processed |
| `x402_proxy_request_duration_seconds` | Histogram | status | Request latency |
| `x402_proxy_rate_limited_total` | Counter | layer | Rate-limited requests |
| `x402_proxy_payments_total` | Counter | status | Payment attempts |
| `x402_proxy_tracked_ips` | Gauge | - | IPs being tracked |
| `x402_proxy_active_connections` | Gauge | - | Active connections |

---

## Metric Definitions

### Request Counter

```rust
pub static REQUESTS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "x402_proxy_requests_total",
        "Total number of requests processed",
        &["status", "method"]
    ).expect("Failed to register metric")
});
```

**Labels**:
- `status`: `"allowed"`, `"rate_limited"`, `"paid"`
- `method`: `"GET"`, `"POST"`, etc.

### Request Duration Histogram

```rust
pub static REQUEST_DURATION: Lazy<HistogramVec> = Lazy::new(|| {
    register_histogram_vec!(
        "x402_proxy_request_duration_seconds",
        "Request duration in seconds",
        &["status"],
        vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
    ).expect("Failed to register metric")
});
```

### Rate Limit Counter

```rust
pub static RATE_LIMITED_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "x402_proxy_rate_limited_total",
        "Total number of rate-limited requests",
        &["layer"]  // "ip" or "global"
    ).expect("Failed to register metric")
});
```

### Payment Counter

```rust
pub static PAYMENTS_TOTAL: Lazy<CounterVec> = Lazy::new(|| {
    register_counter_vec!(
        "x402_proxy_payments_total",
        "Total number of x402 payment attempts",
        &["status"]  // "verified", "settled", "failed"
    ).expect("Failed to register metric")
});
```

### Gauge Metrics

```rust
pub static TRACKED_IPS: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "x402_proxy_tracked_ips",
        "Current number of IPs being tracked by rate limiter"
    ).expect("Failed to register metric")
});

pub static ACTIVE_CONNECTIONS: Lazy<Gauge> = Lazy::new(|| {
    register_gauge!(
        "x402_proxy_active_connections",
        "Current number of active connections"
    ).expect("Failed to register metric")
});
```

---

## Helper Functions

### `init_metrics()`

Forces lazy initialization of all metrics. Called at startup.

```rust
pub fn init_metrics() {
    Lazy::force(&REQUESTS_TOTAL);
    Lazy::force(&REQUEST_DURATION);
    Lazy::force(&RATE_LIMITED_TOTAL);
    Lazy::force(&PAYMENTS_TOTAL);
    Lazy::force(&TRACKED_IPS);
    Lazy::force(&ACTIVE_CONNECTIONS);
}
```

### `gather_metrics() -> String`

Generates Prometheus text format output.

```rust
pub fn gather_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap_or_default()
}
```

**Output Example**:
```
# HELP x402_proxy_requests_total Total number of requests processed
# TYPE x402_proxy_requests_total counter
x402_proxy_requests_total{method="GET",status="allowed"} 1234
x402_proxy_requests_total{method="GET",status="rate_limited"} 56
x402_proxy_requests_total{method="POST",status="paid"} 7
```

### Recording Functions

```rust
pub fn record_request(status: &str, method: &str) {
    REQUESTS_TOTAL.with_label_values(&[status, method]).inc();
}

pub fn record_rate_limited(layer: &str) {
    RATE_LIMITED_TOTAL.with_label_values(&[layer]).inc();
}

pub fn record_payment(status: &str) {
    PAYMENTS_TOTAL.with_label_values(&[status]).inc();
}

pub fn set_tracked_ips(count: usize) {
    TRACKED_IPS.set(count as f64);
}
```

---

## Integration with Proxy

In `proxy.rs`:

```rust
async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) 
    -> Result<bool> 
{
    let method = session.req_header().method.as_str();
    
    // Update tracked IPs gauge
    metrics::set_tracked_ips(self.rate_limiter.stats().tracked_ips);
    
    match self.rate_limiter.check(&ctx.client_ip) {
        RateLimitResult::Allowed => {
            metrics::record_request("allowed", method);
            Ok(false)
        }
        RateLimitResult::IpLimitExceeded => {
            metrics::record_request("rate_limited", method);
            metrics::record_rate_limited("ip");
            self.build_402_response(session, ctx).await
        }
        RateLimitResult::GlobalLimitExceeded => {
            metrics::record_request("rate_limited", method);
            metrics::record_rate_limited("global");
            self.build_402_response(session, ctx).await
        }
    }
}
```

---

## Prometheus Configuration

```yaml
# prometheus.yml
scrape_configs:
  - job_name: 'x402-proxy'
    static_configs:
      - targets: ['proxy:8080']
    metrics_path: '/metrics'
    scrape_interval: 15s
```

---

## Example Grafana Queries

### Request Rate by Status
```promql
sum(rate(x402_proxy_requests_total[5m])) by (status)
```

### Rate Limited Percentage
```promql
sum(rate(x402_proxy_rate_limited_total[5m])) / sum(rate(x402_proxy_requests_total[5m])) * 100
```

### Payment Success Rate
```promql
sum(rate(x402_proxy_payments_total{status="settled"}[5m])) / 
sum(rate(x402_proxy_payments_total[5m])) * 100
```

### Active IPs
```promql
x402_proxy_tracked_ips
```
