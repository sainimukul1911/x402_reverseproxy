# New Features Documentation

This document describes features added to x402 Reverse Proxy beyond the initial base implementation.

## 1. Multi-Upstream Load Balancing

The proxy now supports multiple upstream servers to distribute traffic and provide redundancy.

### Configuration
In `config.toml`:
```toml
[upstream]
urls = ["http://server1:8080", "http://server2:8080"]
selection_mode = "round_robin" # Default
```

### Implementation
- Uses `Pingora`'s `LoadBalancer` with `RoundRobin` selection.
- Traffic is distributed evenly across healthy upstreams.
- If no upstreams are defined, it defaults to localhost to prevent startup failure (fail-safe).

## 2. Dynamic Rate Limiting

Rate limits can now be updated at runtime without restarting the server.

### Implementation
- Limits are stored in `Arc<AtomicU64>` for thread-safe concurrent access.
- Updates are instantaneous and apply to all new requests.

### Admin API
- **Endpoint**: `POST /admin/ratelimits`
- **Query Params**: `per_ip` (int), `global` (int)
- **Example**:
  ```bash
  curl -X POST "http://localhost:8080/admin/ratelimits?per_ip=50&global=5000"
  ```
- **Response**: JSON confirmation of new limits.

## 3. IP Management (Allowlist/Blocklist)

Granular control over client access.

### Configuration
In `config.toml`:
```toml
[rate_limits]
allowlist = ["192.168.1.5"]      # Bypass all checks
blocklist = ["10.0.0.1"]         # Always blocked (403 Forbidden)
```

### Logic
1. **Blocklist Check**: If IP is in blocklist, return `403 Forbidden` immediately.
2. **Allowlist Check**: If IP is in allowlist, bypass payment and rate limits.
3. **Payment/Rate Limit**: Normal flow for others.

## 4. Enhanced Admin API

The proxy exposes administrative endpoints on the same port (protected by path prefix):

- `/health`, `/ready`, `/metrics`: Standard monitoring.
- `/admin/ratelimits`: Dynamic configuration.
