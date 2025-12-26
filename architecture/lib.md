# lib.rs - Library Entry Point

Re-exports all public modules for use as a library.

---

## Overview

This is the root of the library crate. It:
1. Declares all modules
2. Re-exports commonly used types
3. Enables the proxy to be used as a library

---

## Module Declarations

```rust
pub mod config;
pub mod health;
pub mod metrics;
pub mod proxy;
pub mod rate_limiter;
pub mod x402;
```

---

## Re-exports

```rust
pub use config::Config;
pub use proxy::X402Proxy;
pub use rate_limiter::{RateLimitResult, RateLimiter};
pub use x402::X402Handler;
```

This allows users to write:
```rust
use x402_reverseproxy::{Config, X402Proxy, RateLimiter};
```

Instead of:
```rust
use x402_reverseproxy::config::Config;
use x402_reverseproxy::proxy::X402Proxy;
use x402_reverseproxy::rate_limiter::RateLimiter;
```

---

## Module Dependency Graph

```
                    lib.rs
                       │
        ┌──────────────┼──────────────┐
        │              │              │
        ▼              ▼              ▼
    config.rs      proxy.rs      metrics.rs
        │              │              │
        │     ┌────────┼────────┐     │
        │     │        │        │     │
        │     ▼        ▼        ▼     │
        │  rate_    x402.rs  health.rs│
        │  limiter.rs     │           │
        │     │           │           │
        └─────┴─────┬─────┴───────────┘
                    │
              (all use)
                    │
                    ▼
               main.rs
```

---

## Using as a Library

The crate can be used as a library in other projects:

```rust
// In another Rust project
use x402_reverseproxy::{Config, X402Proxy, RateLimiter};

fn main() {
    let config = Config::from_file("config.toml").unwrap();
    let proxy = X402Proxy::new(config);
    
    // Use proxy in your own Pingora server
}
```

**Cargo.toml**:
```toml
[dependencies]
x402_reverseproxy = { path = "../x402_reverseproxy" }
# or from crates.io:
# x402_reverseproxy = "0.1"
```
