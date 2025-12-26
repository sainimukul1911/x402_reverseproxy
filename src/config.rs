//! Configuration module for x402 reverse proxy.
//!
//! Parses the TOML configuration file and provides typed access to all settings.

use serde::Deserialize;
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during configuration loading.
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
}

/// Root configuration structure.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub upstream: UpstreamConfig,
    pub rate_limits: RateLimitConfig,
    pub x402: X402Config,
}

/// Server binding configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    /// Address to bind the proxy server (e.g., "0.0.0.0:8080")
    pub bind: String,

    /// Number of worker threads (defaults to CPU count)
    #[serde(default)]
    pub workers: Option<usize>,

    /// TLS configuration (optional)
    #[serde(default)]
    pub tls: Option<TlsConfig>,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,
}

/// TLS configuration for HTTPS
#[derive(Debug, Clone, Deserialize)]
pub struct TlsConfig {
    /// Path to certificate file (PEM format)
    pub cert_path: String,

    /// Path to private key file (PEM format)
    pub key_path: String,
}

/// Logging configuration
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// Log format: "json" or "pretty" (default: "pretty")
    #[serde(default = "default_log_format")]
    pub format: String,

    /// Log level: "trace", "debug", "info", "warn", "error" (default: "info")
    #[serde(default = "default_log_level")]
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: default_log_format(),
            level: default_log_level(),
        }
    }
}

fn default_log_format() -> String {
    "pretty".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Upstream server configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamConfig {
    /// URL of the upstream server to proxy to
    pub url: String,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    30
}

/// Rate limiting configuration (dual-layer defense).
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    /// Layer 1: Per-IP rate limit (requests per second)
    /// If a single IP exceeds this, block only that IP with 402
    pub per_ip_requests_per_second: u32,

    /// Layer 2: Global rate limit (requests per second)
    /// If total server load exceeds this, block all non-paying traffic
    pub global_requests_per_second: u32,

    /// Time window for rate limiting in seconds
    #[serde(default = "default_window")]
    pub window_seconds: u64,
}

fn default_window() -> u64 {
    1
}

/// x402 payment configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct X402Config {
    /// URL of the x402 facilitator service
    pub facilitator_url: String,

    /// Recipient wallet address for payments
    pub recipient_address: String,

    /// Amount to charge per request (in token units, e.g., USDC)
    pub amount: String,

    /// Blockchain network (e.g., "base", "base-sepolia")
    pub network: String,

    /// Token contract address (e.g., USDC on Base)
    pub token: String,

    /// Description shown to clients in payment requirements
    #[serde(default = "default_description")]
    pub description: String,
}

fn default_description() -> String {
    "API access payment".to_string()
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let toml_str = r#"
            [server]
            bind = "0.0.0.0:8080"
            
            [upstream]
            url = "http://localhost:3000"
            
            [rate_limits]
            per_ip_requests_per_second = 10
            global_requests_per_second = 1000
            
            [x402]
            facilitator_url = "https://x402.org/facilitator"
            recipient_address = "0x1234567890abcdef"
            amount = "0.001"
            network = "base-sepolia"
            token = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.bind, "0.0.0.0:8080");
        assert_eq!(config.rate_limits.per_ip_requests_per_second, 10);
        assert_eq!(config.x402.network, "base-sepolia");
    }
}
