# rate_limiter.rs - Dual-Layer Rate Limiting

Implements per-IP and global rate limiting using `pingora-limits`.

---

## Overview

This module provides a **dual-layer defense system**:

| Layer | Purpose | Trigger |
|-------|---------|---------|
| **Layer 1** | Per-IP limiting | Single abusive IP |
| **Layer 2** | Global limiting | DDoS / mass abuse |

---

## How It Works

```
Request comes in
       │
       ▼
┌──────────────────────────┐
│ Layer 2: Global Limit    │ ◄── Checked FIRST (DDoS protection)
│ Total RPS across all IPs │
└────────────┬─────────────┘
             │ allowed
             ▼
┌──────────────────────────┐
│ Layer 1: Per-IP Limit    │ ◄── Checked SECOND (abuse protection)
│ RPS for this specific IP │
└────────────┬─────────────┘
             │ allowed
             ▼
       Forward request
```

**Why Layer 2 first?** During a DDoS attack, we want to reject ALL traffic quickly rather than checking thousands of individual IP limits.

---

## Structs

### `RateLimitResult`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    Allowed,              // Request can proceed
    IpLimitExceeded,      // This IP hit their limit
    GlobalLimitExceeded,  // Server-wide limit hit
}
```

### `RateLimiter`

```rust
pub struct RateLimiter {
    ip_limiters: Arc<RwLock<HashMap<String, Rate>>>,  // Per-IP
    global_limiter: Arc<Rate>,                         // Global
    per_ip_limit: f64,                                 // Threshold
    global_limit: f64,                                 // Threshold
    window: Duration,                                  // Time window
}
```

### `RateLimiterStats`

```rust
#[derive(Debug, Clone)]
pub struct RateLimiterStats {
    pub tracked_ips: usize,   // Number of IPs being tracked
    pub global_rate: f64,     // Current global request rate
}
```

---

## Key Methods

### `RateLimiter::new(per_ip_rps, global_rps, window_seconds)`

```rust
pub fn new(per_ip_rps: u32, global_rps: u32, window_seconds: u64) -> Self {
    let window = Duration::from_secs(window_seconds);
    
    Self {
        ip_limiters: Arc::new(RwLock::new(HashMap::new())),
        global_limiter: Arc::new(Rate::new(window)),
        per_ip_limit: per_ip_rps as f64,
        global_limit: global_rps as f64,
        window,
    }
}
```

---

### `check(client_ip) -> RateLimitResult`

Main entry point. Checks both layers and returns result.

```rust
pub fn check(&self, client_ip: &str) -> RateLimitResult {
    // Layer 2: Global check (DDoS protection)
    self.global_limiter.observe(&"global", 1);
    let global_rate = self.global_limiter.rate(&"global");
    
    if global_rate > self.global_limit {
        return RateLimitResult::GlobalLimitExceeded;
    }
    
    // Layer 1: Per-IP check
    {
        let mut ip_limiters = self.ip_limiters.write();
        let limiter = ip_limiters
            .entry(client_ip.to_string())
            .or_insert_with(|| Rate::new(self.window));
        
        limiter.observe(&client_ip, 1);
        let ip_rate = limiter.rate(&client_ip);
        
        if ip_rate > self.per_ip_limit {
            return RateLimitResult::IpLimitExceeded;
        }
    }
    
    RateLimitResult::Allowed
}
```

---

### `stats() -> RateLimiterStats`

Returns current statistics for monitoring.

```rust
pub fn stats(&self) -> RateLimiterStats {
    let ip_limiters = self.ip_limiters.read();
    
    RateLimiterStats {
        tracked_ips: ip_limiters.len(),
        global_rate: self.global_limiter.rate(&"global"),
    }
}
```

---

### `cleanup_stale_entries()`

Removes IPs that haven't made requests recently. Called periodically to free memory.

```rust
pub fn cleanup_stale_entries(&self) {
    let mut ip_limiters = self.ip_limiters.write();
    ip_limiters.retain(|ip, limiter| {
        limiter.rate(ip) > 0.0  // Keep only active IPs
    });
}
```

---

## Thread Safety

The `RateLimiter` is designed for concurrent access:

| Component | Synchronization |
|-----------|-----------------|
| `ip_limiters` | `Arc<RwLock<HashMap>>` |
| `global_limiter` | `Arc<Rate>` (internally thread-safe) |

**Clone Behavior**: Cloning a `RateLimiter` shares the underlying state:

```rust
impl Clone for RateLimiter {
    fn clone(&self) -> Self {
        Self {
            ip_limiters: Arc::clone(&self.ip_limiters),
            global_limiter: Arc::clone(&self.global_limiter),
            // ... copies of thresholds
        }
    }
}
```

---

## pingora-limits Integration

Uses Pingora's `Rate` estimator for sliding window rate calculation:

```rust
use pingora_limits::rate::Rate;

// Create a rate estimator with 1-second window
let rate = Rate::new(Duration::from_secs(1));

// Record an event (returns event count in this window)
rate.observe(&"key", 1);

// Get estimated rate (events per second)
let rps = rate.rate(&"key");
```

---

## Example Scenarios

### Normal Traffic
```
IP 10.0.0.1: 5 req/s (limit: 10) → Allowed
IP 10.0.0.2: 3 req/s (limit: 10) → Allowed
Global: 8 req/s (limit: 1000) → Allowed
```

### Single Abuser
```
IP 10.0.0.1: 15 req/s (limit: 10) → IpLimitExceeded (402)
IP 10.0.0.2: 3 req/s (limit: 10) → Allowed
Global: 18 req/s (limit: 1000) → Allowed
```

### DDoS Attack
```
Many IPs: 2 req/s each
Global: 5000 req/s (limit: 1000) → GlobalLimitExceeded (402 for everyone)
```

---

## Metrics Integration

The proxy records rate limit events:

```rust
// In proxy.rs
match self.rate_limiter.check(&ctx.client_ip) {
    RateLimitResult::IpLimitExceeded => {
        metrics::record_rate_limited("ip");
    }
    RateLimitResult::GlobalLimitExceeded => {
        metrics::record_rate_limited("global");
    }
    _ => {}
}

// Also updates tracked IP gauge
metrics::set_tracked_ips(self.rate_limiter.stats().tracked_ips);
```
