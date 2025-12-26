# proxy.rs - Core Proxy Logic

The heart of the x402 reverse proxy, implementing Pingora's `ProxyHttp` trait.

---

## Overview

This module is the central orchestrator that:
1. Receives incoming HTTP requests
2. Checks for health/admin endpoints
3. Validates x402 payment signatures
4. Enforces rate limits
5. Forwards allowed requests to upstream
6. Settles payments on successful responses

---

## Structs

### `RequestContext`

Per-request state carried through the request lifecycle.

```rust
pub struct RequestContext {
    pub client_ip: String,                    // Extracted client IP
    pub payment: Option<PaymentPayload>,      // Parsed x402 payment
    pub paid: bool,                           // Whether request used payment
    pub path: String,                         // Request path
}
```

### `X402Proxy`

Main proxy struct holding shared state.

```rust
pub struct X402Proxy {
    config: Arc<Config>,          // Shared configuration
    rate_limiter: RateLimiter,    // Dual-layer rate limiter
    x402_handler: X402Handler,    // Payment protocol handler
}
```

---

## Key Methods

### `X402Proxy::new(config)`

Creates a new proxy instance with all components initialized.

```rust
pub fn new(config: Config) -> Self {
    // 1. Create rate limiter with config values
    let rate_limiter = RateLimiter::new(
        config.rate_limits.per_ip_requests_per_second,
        config.rate_limits.global_requests_per_second,
        config.rate_limits.window_seconds,
    );
    
    // 2. Create x402 handler with payment config
    let x402_handler = X402Handler::new(
        config.x402.facilitator_url,
        config.x402.recipient_address,
        // ... other fields
    );
    
    // 3. Return assembled proxy
    Self { config, rate_limiter, x402_handler }
}
```

---

### `get_client_ip(session)`

Extracts the real client IP from headers or connection.

**Priority Order**:
1. `X-Forwarded-For` header (first IP in chain)
2. `X-Real-IP` header
3. Direct peer address
4. Fallback: `"unknown"`

```rust
fn get_client_ip(&self, session: &Session) -> String {
    if let Some(xff) = session.req_header().headers.get("X-Forwarded-For") {
        // Take first IP: "10.0.0.1, 10.0.0.2" -> "10.0.0.1"
        return xff.split(',').next().trim().to_string();
    }
    // ... fallback logic
}
```

---

### `check_payment(session, ctx)`

Validates x402 payment signature with facilitator.

**Flow**:
```
1. Get PAYMENT-SIGNATURE header
2. Base64 decode → JSON parse
3. Call facilitator /verify endpoint
4. If valid: set ctx.paid = true, return true
5. If invalid: log warning, return false
```

```rust
async fn check_payment(&self, session: &Session, ctx: &mut RequestContext) -> bool {
    let header = session.req_header().headers.get("PAYMENT-SIGNATURE")?;
    let payment = self.x402_handler.parse_payment_signature(header)?;
    
    match self.x402_handler.verify_payment(&payment, &ctx.path).await {
        Ok(true) => {
            ctx.payment = Some(payment);
            ctx.paid = true;
            true
        }
        _ => false
    }
}
```

---

### `build_402_response(session, ctx)`

Generates HTTP 402 Payment Required response.

**Response Structure**:
```http
HTTP/1.1 402 Payment Required
PAYMENT-REQUIRED: <base64-encoded-requirements>
Content-Type: application/json

{
    "error": "payment_required",
    "message": "Rate limit exceeded. Pay to continue.",
    "paymentRequirements": { ... }
}
```

---

## ProxyHttp Trait Implementation

### `new_ctx()`

Creates fresh `RequestContext` for each request.

### `request_filter(session, ctx)` - Main Logic

This is where the magic happens. Called for every incoming request.

```rust
async fn request_filter(&self, session: &mut Session, ctx: &mut Self::CTX) 
    -> Result<bool> 
{
    // Step 0: Extract request info
    ctx.client_ip = self.get_client_ip(session);
    ctx.path = session.req_header().uri.path().to_string();
    let method = session.req_header().method.as_str();

    // Step 1: Health endpoints bypass everything
    if health::is_admin_path(&ctx.path) {
        // Handle /health, /ready, /metrics directly
        return self.handle_admin(session, ctx);
    }

    // Step 2: Check for payment - bypasses rate limits
    if self.check_payment(session, ctx).await {
        metrics::record_request("paid", method);
        return Ok(false); // Continue to upstream
    }

    // Step 3: Check rate limits
    match self.rate_limiter.check(&ctx.client_ip) {
        RateLimitResult::Allowed => {
            metrics::record_request("allowed", method);
            Ok(false) // Continue to upstream
        }
        RateLimitResult::IpLimitExceeded => {
            metrics::record_rate_limited("ip");
            self.build_402_response(session, ctx).await
        }
        RateLimitResult::GlobalLimitExceeded => {
            metrics::record_rate_limited("global");
            self.build_402_response(session, ctx).await
        }
    }
}
```

**Return Value**:
- `Ok(false)` → Continue to upstream
- `Ok(true)` → Response already sent (don't forward)

### `upstream_peer(session, ctx)`

Determines which upstream server to connect to.

```rust
async fn upstream_peer(&self, ...) -> Result<Box<HttpPeer>> {
    let (host, port, tls) = parse_upstream_url(&self.config.upstream.url);
    Ok(Box::new(HttpPeer::new((host, port), tls, host)))
}
```

### `response_filter(session, response, ctx)`

Called after upstream responds. Used to settle payments.

```rust
async fn response_filter(&self, ..., ctx: &mut Self::CTX) -> Result<()> {
    // If this was a paid request, settle the payment
    if let Some(ref payment) = ctx.payment {
        match self.x402_handler.settle_payment(payment, &ctx.path).await {
            Ok(settle_response) => {
                // Add PAYMENT-RESPONSE header
                response.insert_header("PAYMENT-RESPONSE", ...);
            }
            Err(e) => warn!("Settlement failed: {}", e)
        }
    }
    Ok(())
}
```

---

## Helper Functions

### `parse_upstream_url(url) -> (host, port, tls)`

Parses upstream URL into components.

```rust
// "https://api.example.com:8443/path"
//   → ("api.example.com", 8443, true)

// "http://localhost:3000"
//   → ("localhost", 3000, false)
```

---

## Request Flow Diagram

```
Incoming Request
       │
       ▼
┌──────────────────┐
│ request_filter() │
└────────┬─────────┘
         │
    ┌────▼────┐
    │ Admin?  │──yes──▶ Return /health, /metrics
    └────┬────┘
         │ no
    ┌────▼────┐
    │ Paid?   │──yes──▶ Bypass limits, forward
    └────┬────┘
         │ no
    ┌────▼────────┐
    │ Rate limit? │
    └────┬────────┘
         │
    ┌────▼────┐
    │ Allowed │──yes──▶ Forward to upstream
    └────┬────┘
         │ no
         ▼
    Return 402
```

---

## Metrics Integration

The proxy records metrics at each decision point:

| Event | Metric |
|-------|--------|
| Request allowed | `requests_total{status="allowed"}` |
| Paid request | `requests_total{status="paid"}` |
| IP rate limited | `rate_limited_total{layer="ip"}` |
| Global rate limited | `rate_limited_total{layer="global"}` |
