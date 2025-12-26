# main.rs - Binary Entry Point

The executable entry point that bootstraps the proxy server.

---

## Overview

This module:
1. Parses CLI arguments
2. Loads configuration
3. Initializes logging (JSON or pretty)
4. Initializes metrics
5. Creates Pingora server
6. Adds TLS listener (if configured)
7. Runs server forever

---

## CLI Arguments

```rust
#[derive(Parser, Debug)]
#[command(name = "x402-proxy")]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config.toml")]
    config: PathBuf,

    /// Enable debug logging (overrides config)
    #[arg(short, long)]
    debug: bool,

    /// Use JSON log format (overrides config)
    #[arg(long)]
    json: bool,

    /// Run in daemon mode (background)
    #[arg(short = 'D', long)]
    daemon: bool,
}
```

**Usage**:
```bash
x402-proxy --config /etc/x402/config.toml
x402-proxy --debug --json
x402-proxy -D  # Daemon mode
```

---

## Startup Flow

```rust
fn main() {
    // 1. Parse CLI arguments
    let args = Args::parse();

    // 2. Load configuration (needed for logging config)
    let config = Config::from_file(&args.config)?;

    // 3. Initialize logging based on config + CLI
    init_logging(&config, &args);

    // 4. Initialize Prometheus metrics
    metrics::init_metrics();

    // 5. Create Pingora server
    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    // 6. Create proxy service
    let proxy = X402Proxy::new(config.clone());
    let mut proxy_service = http_proxy_service(&server.configuration, proxy);

    // 7. Add listener (HTTP or HTTPS)
    if let Some(tls_config) = &config.server.tls {
        proxy_service.add_tls(&bind_addr, &tls_config.cert_path, &tls_config.key_path);
    } else {
        proxy_service.add_tcp(&bind_addr);
    }

    // 8. Mark as ready
    health::set_ready(true);

    // 9. Run forever
    server.run_forever();
}
```

---

## Logging Initialization

```rust
fn init_logging(config: &Config, args: &Args) {
    // CLI overrides config
    let level = if args.debug { "debug" } else { &config.server.logging.level };
    let use_json = args.json || config.server.logging.format == "json";

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level));

    if use_json {
        // JSON structured logging for production
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer()
                .json()
                .with_span_events(FmtSpan::CLOSE)
                .with_current_span(true)
                .with_target(true)
                .with_file(true)
                .with_line_number(true))
            .init();
    } else {
        // Pretty logging for development
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().pretty())
            .init();
    }
}
```

**JSON Log Output**:
```json
{
    "timestamp": "2024-01-15T10:30:00.000Z",
    "level": "INFO",
    "target": "x402_reverseproxy::proxy",
    "message": "Incoming request",
    "client_ip": "192.168.1.1",
    "path": "/api/data",
    "method": "GET"
}
```

---

## TLS Configuration

When TLS is enabled in `config.toml`:

```toml
[server.tls]
cert_path = "/etc/ssl/certs/server.pem"
key_path = "/etc/ssl/private/server.key"
```

The proxy will:
1. Terminate TLS at the proxy
2. Forward requests to upstream over HTTP/HTTPS (based on upstream URL)

---

## Startup Logging

```rust
info!(
    version = env!("CARGO_PKG_VERSION"),
    config_path = ?args.config,
    "x402 Reverse Proxy starting"
);

info!(
    bind = %config.server.bind,
    upstream = %config.upstream.url,
    per_ip_limit = config.rate_limits.per_ip_requests_per_second,
    global_limit = config.rate_limits.global_requests_per_second,
    network = %config.x402.network,
    tls_enabled = config.server.tls.is_some(),
    "Configuration loaded"
);
```

---

## Integration Points

| Component | Purpose |
|-----------|---------|
| `Config::from_file()` | Load TOML configuration |
| `metrics::init_metrics()` | Initialize Prometheus metrics |
| `X402Proxy::new(config)` | Create proxy instance |
| `health::set_ready(true)` | Mark server as ready |
| `server.run_forever()` | Start Pingora event loop |
