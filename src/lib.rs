//! x402 Reverse Proxy Library
//!
//! A high-performance reverse proxy built on Pingora with:
//! - Dual-layer rate limiting (per-IP + global)
//! - x402 micropayment wall for API monetization
//! - Sidecar pattern for any upstream application

pub mod config;
pub mod health;
pub mod metrics;
pub mod proxy;
pub mod rate_limiter;
pub mod x402;

pub use config::Config;
pub use proxy::X402Proxy;
pub use rate_limiter::{RateLimitResult, RateLimiter};
pub use x402::X402Handler;

