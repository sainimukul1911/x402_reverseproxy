# health.rs - Health & Admin Endpoints

Provides observability endpoints that bypass rate limiting.

---

## Overview

This module handles special administrative endpoints:
- `/health` - Liveness check
- `/ready` - Readiness check  
- `/metrics` - Prometheus metrics

These endpoints are handled **before** rate limiting to ensure they're always accessible for monitoring.

---

## Endpoints

| Path | Status OK | Status Error | Purpose |
|------|-----------|--------------|---------|
| `/health`, `/_health` | 200 | 503 | Is the proxy running? |
| `/ready`, `/_ready` | 200 | 503 | Is the proxy ready to accept traffic? |
| `/metrics`, `/_metrics` | 200 | - | Prometheus metrics |

---

## Global State

Uses atomic booleans for thread-safe health state:

```rust
static IS_HEALTHY: AtomicBool = AtomicBool::new(true);
static IS_READY: AtomicBool = AtomicBool::new(false);

pub fn set_ready(ready: bool) {
    IS_READY.store(ready, Ordering::SeqCst);
}

pub fn set_healthy(healthy: bool) {
    IS_HEALTHY.store(healthy, Ordering::SeqCst);
}

pub fn is_healthy() -> bool {
    IS_HEALTHY.load(Ordering::SeqCst)
}

pub fn is_ready() -> bool {
    IS_READY.load(Ordering::SeqCst)
}
```

---

## Response Structures

### Health Response

```rust
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,  // "healthy" | "unhealthy"
    pub version: &'static str, // From Cargo.toml
}
```

**Example**:
```json
{
    "status": "healthy",
    "version": "0.1.0"
}
```

### Ready Response

```rust
#[derive(Serialize)]
pub struct ReadyResponse {
    pub ready: bool,
    pub checks: ReadinessChecks,
}

#[derive(Serialize)]
pub struct ReadinessChecks {
    pub rate_limiter: bool,
    pub upstream: bool,
}
```

**Example**:
```json
{
    "ready": true,
    "checks": {
        "rate_limiter": true,
        "upstream": true
    }
}
```

---

## Key Functions

### `is_admin_path(path) -> bool`

Checks if a path is an admin endpoint.

```rust
pub fn is_admin_path(path: &str) -> bool {
    matches!(path, 
        "/health" | "/ready" | "/metrics" | 
        "/_health" | "/_ready" | "/_metrics"
    )
}
```

---

### `handle_admin_request(path) -> Option<(status, body, content_type)>`

Generates response for admin endpoints.

```rust
pub fn handle_admin_request(path: &str) -> Option<(u16, String, &'static str)> {
    match path {
        "/health" | "/_health" => {
            let status = if is_healthy() { 200 } else { 503 };
            let response = HealthResponse {
                status: if is_healthy() { "healthy" } else { "unhealthy" },
                version: env!("CARGO_PKG_VERSION"),
            };
            Some((status, serde_json::to_string(&response).unwrap(), "application/json"))
        }
        
        "/ready" | "/_ready" => {
            let ready = is_ready();
            let status = if ready { 200 } else { 503 };
            let response = ReadyResponse {
                ready,
                checks: ReadinessChecks {
                    rate_limiter: true,
                    upstream: ready,
                },
            };
            Some((status, serde_json::to_string(&response).unwrap(), "application/json"))
        }
        
        "/metrics" | "/_metrics" => {
            let metrics_output = metrics::gather_metrics();
            Some((200, metrics_output, "text/plain; version=0.0.4"))
        }
        
        _ => None,
    }
}
```

---

### `build_admin_response(status, body, content_type) -> Result<(ResponseHeader, Vec<u8>)>`

Builds the HTTP response for admin endpoints.

```rust
pub fn build_admin_response(
    status: u16, 
    body: String, 
    content_type: &str
) -> Result<(ResponseHeader, Vec<u8>)> {
    let mut resp = ResponseHeader::build(status, None)?;
    resp.insert_header("Content-Type", content_type)?;
    resp.insert_header("Content-Length", body.len().to_string())?;
    resp.insert_header("Cache-Control", "no-cache, no-store")?;
    Ok((resp, body.into_bytes()))
}
```

---

## Integration with Proxy

In `proxy.rs`:

```rust
async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) 
    -> Result<bool> 
{
    ctx.path = session.req_header().uri.path().to_string();
    
    // Check for admin endpoints FIRST (before rate limiting)
    if health::is_admin_path(&ctx.path) {
        if let Some((status, body, content_type)) = health::handle_admin_request(&ctx.path) {
            let (resp, body_bytes) = health::build_admin_response(status, body, content_type)?;
            session.write_response_header(Box::new(resp), false).await?;
            session.write_response_body(Some(body_bytes.into()), true).await?;
            return Ok(true); // Don't forward to upstream
        }
    }
    
    // ... continue with rate limiting
}
```

In `main.rs`:

```rust
fn main() {
    // ... setup ...
    
    // Mark server as ready after initialization
    health::set_ready(true);
    
    info!("Health endpoints: /health, /ready, /metrics");
    server.run_forever();
}
```

---

## Usage Examples

### Kubernetes Probes

```yaml
livenessProbe:
  httpGet:
    path: /health
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 10

readinessProbe:
  httpGet:
    path: /ready
    port: 8080
  initialDelaySeconds: 5
  periodSeconds: 5
```

### Load Balancer Health Check

```bash
curl -f http://localhost:8080/health || exit 1
```

### Prometheus Scraping

```yaml
scrape_configs:
  - job_name: 'x402-proxy'
    static_configs:
      - targets: ['proxy:8080']
    metrics_path: '/metrics'
```
