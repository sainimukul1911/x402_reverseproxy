# config.rs - Configuration Module

Handles TOML parsing and provides typed access to all configuration settings.

---

## Overview

This module:
1. Defines strongly-typed configuration structures
2. Provides sensible defaults for optional fields
3. Validates configuration on load
4. Exposes a single `Config::from_file()` entry point

---

## Configuration Hierarchy

```toml
Config
├── server: ServerConfig
│   ├── bind: String            # "0.0.0.0:8080"
│   ├── workers: Option<usize>  # CPU threads
│   ├── tls: Option<TlsConfig>
│   │   ├── cert_path: String
│   │   └── key_path: String
│   └── logging: LoggingConfig
│       ├── format: String      # "json" | "pretty"
│       └── level: String       # "info" | "debug" | etc
├── upstream: UpstreamConfig
│   ├── url: String             # "http://localhost:3000"
│   └── timeout_seconds: u64    # default: 30
├── rate_limits: RateLimitConfig
│   ├── per_ip_requests_per_second: u32
│   ├── global_requests_per_second: u32
│   └── window_seconds: u64     # default: 1
└── x402: X402Config
    ├── facilitator_url: String
    ├── recipient_address: String
    ├── amount: String
    ├── network: String
    ├── token: String
    └── description: String     # default: "API access payment"
```

---

## Structs

### `Config` (Root)

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub upstream: UpstreamConfig,
    pub rate_limits: RateLimitConfig,
    pub x402: X402Config,
}
```

### `ServerConfig`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,                    // Required
    #[serde(default)]
    pub workers: Option<usize>,          // Optional
    #[serde(default)]
    pub tls: Option<TlsConfig>,          // Optional
    #[serde(default)]
    pub logging: LoggingConfig,          // Has defaults
}
```

### `TlsConfig`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct TlsConfig {
    pub cert_path: String,   // Path to PEM certificate
    pub key_path: String,    // Path to PEM private key
}
```

### `LoggingConfig`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_format")]  // "pretty"
    pub format: String,
    #[serde(default = "default_log_level")]   // "info"
    pub level: String,
}

// Custom Default impl to ensure correct values
impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            format: "pretty".to_string(),
            level: "info".to_string(),
        }
    }
}
```

### `UpstreamConfig`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamConfig {
    pub url: String,
    #[serde(default = "default_timeout")]  // 30
    pub timeout_seconds: u64,
}
```

### `RateLimitConfig`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitConfig {
    pub per_ip_requests_per_second: u32,     // Layer 1
    pub global_requests_per_second: u32,     // Layer 2
    #[serde(default = "default_window")]     // 1
    pub window_seconds: u64,
}
```

### `X402Config`

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct X402Config {
    pub facilitator_url: String,
    pub recipient_address: String,
    pub amount: String,
    pub network: String,
    pub token: String,
    #[serde(default = "default_description")]
    pub description: String,
}
```

---

## Error Handling

```rust
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    FileRead(#[from] std::io::Error),
    
    #[error("Failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
}
```

---

## Loading Configuration

```rust
impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}
```

**Usage in main.rs**:
```rust
let config = Config::from_file(&args.config)?;
```

---

## Default Values

| Field | Default |
|-------|---------|
| `workers` | `None` (Pingora decides) |
| `tls` | `None` (HTTP only) |
| `logging.format` | `"pretty"` |
| `logging.level` | `"info"` |
| `upstream.timeout_seconds` | `30` |
| `rate_limits.window_seconds` | `1` |
| `x402.description` | `"API access payment"` |

---

## Minimal vs Full Configuration

### Minimal (Required Fields Only)

```toml
[server]
bind = "0.0.0.0:8080"

[upstream]
url = "http://localhost:3000"

[rate_limits]
per_ip_requests_per_second = 10
global_requests_per_second = 1000

[x402]
facilitator_url = "https://x402.org/facilitator"
recipient_address = "0x1234..."
amount = "0.001"
network = "base-sepolia"
token = "0x036CbD..."
```

### Full (All Options)

```toml
[server]
bind = "0.0.0.0:443"
workers = 8

[server.tls]
cert_path = "/etc/ssl/cert.pem"
key_path = "/etc/ssl/key.pem"

[server.logging]
format = "json"
level = "debug"

[upstream]
url = "https://api.internal.com"
timeout_seconds = 60

[rate_limits]
per_ip_requests_per_second = 100
global_requests_per_second = 10000
window_seconds = 5

[x402]
facilitator_url = "https://cdp.coinbase.com/facilitator"
recipient_address = "0xYourWallet"
amount = "0.01"
network = "base"
token = "0xUSDC"
description = "Premium API access"
```

---

## Validation

Currently, validation happens implicitly through serde's parsing. Required fields must be present or deserialization fails.

**Future Enhancement**: Add explicit validation:
- URL format validation
- Port range checks
- Rate limit sanity checks
