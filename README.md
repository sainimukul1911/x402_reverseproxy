# x402 Reverse Proxy

A high-performance reverse proxy built on [Pingora](https://github.com/cloudflare/pingora) with **x402 micropayment wall** for API protection.

## Features

- 🛡️ **Dual-layer rate limiting** - Per-IP and global rate limits
- 💰 **x402 Payment Integration** - Bypass limits with micropayments
- 📊 **Prometheus Metrics** - Built-in observability
- 🏥 **Health Endpoints** - `/health`, `/ready`, `/metrics`
- 🔒 **TLS Support** - HTTPS termination
- 📝 **JSON Logging** - Structured logs for production
- 🐳 **Docker Ready** - Multi-stage Dockerfile included

## Quick Start

### 1. Start the Dummy Python Server (Upstream)

```bash
# Terminal 1: Start the upstream API
python examples/dummy_server.py 3000
```

### 2. Configure the Proxy

Edit `config.toml`:

```toml
[server]
bind = "0.0.0.0:8080"

[upstream]
url = "http://localhost:3000"

[rate_limits]
per_ip_requests_per_second = 5
global_requests_per_second = 100

[x402]
facilitator_url = "https://x402.org/facilitator"
recipient_address = "0xYourWalletAddress"
amount = "0.001"
network = "base-sepolia"
token = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
```

### 3. Run the Proxy

```bash
# Terminal 2: Build and run the proxy
cargo build --release
./target/release/x402-proxy --config config.toml
```

### 4. Test It!

```bash
# Make requests through the proxy
curl http://localhost:8080/api/data
curl http://localhost:8080/api/expensive

# Check health
curl http://localhost:8080/health

# View metrics
curl http://localhost:8080/metrics

# Hammer it to trigger rate limiting
for i in {1..20}; do curl -s http://localhost:8080/api/data | head -1; done
```

After exceeding rate limits, you'll receive a **402 Payment Required** response with x402 payment instructions.

---

## Docker Quick Start

```bash
# Build and run with Docker Compose
docker compose up -d

# With Prometheus + Grafana monitoring
docker compose --profile monitoring up -d

# View logs
docker compose logs -f x402-proxy
```

---

## Configuration Reference

### Server

```toml
[server]
bind = "0.0.0.0:8080"    # Listen address
workers = 4               # Worker threads (optional)

[server.tls]              # Optional: Enable HTTPS
cert_path = "/path/to/cert.pem"
key_path = "/path/to/key.pem"

[server.logging]
format = "json"           # "json" or "pretty"
level = "info"            # trace, debug, info, warn, error
```

### Upstream

```toml
[upstream]
url = "http://localhost:3000"
timeout_seconds = 30
```

### Rate Limits

```toml
[rate_limits]
per_ip_requests_per_second = 10    # Layer 1: Per-IP limit
global_requests_per_second = 1000  # Layer 2: Global limit
window_seconds = 1                 # Time window
```

### x402 Payments

```toml
[x402]
facilitator_url = "https://x402.org/facilitator"
recipient_address = "0xYourWallet"
amount = "0.001"           # Cost per request
network = "base-sepolia"   # Blockchain network
token = "0xUSDC..."        # Token contract
description = "API access"
```

---

## Endpoints

| Endpoint | Description |
|----------|-------------|
| `/health` | Health check (200 OK / 503) |
| `/ready` | Readiness check |
| `/metrics` | Prometheus metrics |

---

## CLI Options

```bash
x402-proxy --help

Options:
  -c, --config <FILE>   Config file path [default: config.toml]
  -d, --debug           Enable debug logging
  --json                Force JSON log format
  -D, --daemon          Run in background
```

---

## Running Tests

```bash
# Run all tests
cargo test

# Run specific test module
cargo test config_tests
cargo test rate_limiter_tests
cargo test x402_tests
cargo test health_tests
cargo test metrics_tests
cargo test integration_tests

# With output
cargo test -- --nocapture
```

---

## How It Works

```
Client Request
     │
     ▼
┌─────────────────────────────────────────────┐
│              x402 Reverse Proxy             │
├─────────────────────────────────────────────┤
│  1. Health endpoint? → Return health/metrics│
│  2. Has payment?     → Verify & bypass      │
│  3. Within limits?   → Forward to upstream  │
│  4. Rate limited?    → Return 402 + x402    │
└─────────────────────────────────────────────┘
     │
     ▼
   Upstream API
```

---

## License

MIT
