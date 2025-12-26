# x402 Reverse Proxy - System Overview

This document explains how all components of the x402 reverse proxy work together. It covers the system architecture, request processing logic, and detailed flow for every scenario the proxy handles.

---

## Table of Contents

1. [What is x402 Reverse Proxy?](#what-is-x402-reverse-proxy)
2. [System Architecture](#system-architecture)
3. [Request Processing Pipeline](#request-processing-pipeline)
4. [Detailed Request Flows](#detailed-request-flows)
5. [Component Integration](#component-integration)
6. [Error Handling Strategy](#error-handling-strategy)

---

## What is x402 Reverse Proxy?

The x402 reverse proxy sits between clients and your API, providing **intelligent traffic protection** with a **payment escape valve**. Unlike traditional rate limiters that simply block excess traffic, x402 offers paying clients the ability to bypass limits using crypto micropayments.

**Core Concept**: Free users get rate-limited. Paying users get unlimited access.

```
┌──────────┐      ┌─────────────────┐      ┌──────────┐
│  Client  │ ───► │  x402 Proxy     │ ───► │ Your API │
│          │      │  (rate limits + │      │          │
│          │ ◄─── │   payments)     │ ◄─── │          │
└──────────┘      └─────────────────┘      └──────────┘
```

---

## System Architecture

The proxy is built from six Rust modules, each with a single responsibility:

| Module | Responsibility |
|--------|---------------|
| `main.rs` | Application entry point, CLI parsing, server bootstrap |
| `config.rs` | TOML configuration parsing and validation |
| `proxy.rs` | Core request handling logic (Pingora ProxyHttp trait) |
| `rate_limiter.rs` | Dual-layer rate limiting (per-IP + global) |
| `x402.rs` | x402 payment protocol (headers, verify, settle) |
| `health.rs` | Admin endpoints (/health, /ready, /metrics) |
| `metrics.rs` | Prometheus metrics export |

### How Components Connect

When the proxy starts, `main.rs` loads the configuration and creates an `X402Proxy` instance. This proxy struct holds references to the rate limiter and x402 handler. Every incoming request flows through the `request_filter` method, which orchestrates all the decision-making.

```
main.rs creates:
├── Config (from TOML file)
├── X402Proxy
│   ├── RateLimiter (created with rate limit config)
│   └── X402Handler (created with payment config)
└── Pingora Server (runs the proxy)
```

---

## Request Processing Pipeline

Every HTTP request goes through a **five-stage pipeline**. The proxy makes decisions at each stage to determine whether to forward the request, block it, or handle it directly.

### Stage 1: Admin Endpoint Check

The first thing the proxy checks is whether the request is for an administrative endpoint like `/health`, `/ready`, or `/metrics`. These endpoints are critical for monitoring and must **always be accessible**, regardless of rate limits or server load.

If the path matches an admin endpoint, the proxy generates the response immediately and returns it to the client. The request never reaches the rate limiter or upstream server.

### Stage 2: Payment Detection

Next, the proxy looks for a `PAYMENT-SIGNATURE` header. This header contains a Base64-encoded payment proof that the client obtained after making a crypto payment. If present, the proxy attempts to verify this payment with the external x402 facilitator service.

A valid payment grants the client **VIP status** for this request—they completely bypass rate limiting and get immediate access to the upstream API.

### Stage 3: Rate Limit Check

If no valid payment was provided, the proxy enforces rate limits. This happens in two layers:

1. **Global Layer (checked first)**: Is the entire server overloaded? This protects against DDoS attacks by rejecting all traffic when the total request rate exceeds the configured threshold.

2. **Per-IP Layer (checked second)**: Is this specific client making too many requests? This blocks individual abusers while allowing other clients through.

If either layer is exceeded, the proxy returns HTTP 402 (Payment Required) with instructions on how to pay for access.

### Stage 4: Upstream Forwarding

If the request passes all checks (or was VIP), the proxy forwards it to the configured upstream server. The proxy acts as a transparent intermediary—the upstream receives the request as if it came directly from the client.

### Stage 5: Response Processing

After the upstream responds, the proxy has one final task. If the request was paid, the proxy contacts the facilitator to **settle** the payment (finalize the transaction). It then adds a `PAYMENT-RESPONSE` header confirming the transaction.

```
Request arrives
       │
       ▼
┌─ Stage 1 ─────────────────────────────────────┐
│ Is this /health, /ready, or /metrics?         │
│ YES → Return admin response immediately       │
│ NO  → Continue to Stage 2                     │
└───────────────────────────────────────────────┘
       │
       ▼
┌─ Stage 2 ─────────────────────────────────────┐
│ Does request have PAYMENT-SIGNATURE header?   │
│ YES → Verify with facilitator                 │
│       Valid?   → Mark as VIP, skip Stage 3    │
│       Invalid? → Log warning, continue        │
│ NO  → Continue to Stage 3                     │
└───────────────────────────────────────────────┘
       │
       ▼
┌─ Stage 3 ─────────────────────────────────────┐
│ Check rate limits (global then per-IP)        │
│ ALLOWED  → Continue to Stage 4                │
│ EXCEEDED → Return 402 with payment info       │
└───────────────────────────────────────────────┘
       │
       ▼
┌─ Stage 4 ─────────────────────────────────────┐
│ Forward request to upstream server            │
│ Wait for response                             │
└───────────────────────────────────────────────┘
       │
       ▼
┌─ Stage 5 ─────────────────────────────────────┐
│ If request was paid → Settle payment          │
│ Add PAYMENT-RESPONSE header if settled        │
│ Forward response to client                    │
└───────────────────────────────────────────────┘
```

---

## Detailed Request Flows

This section walks through every possible scenario the proxy handles.

---

### Scenario 1: Health Check Request

**Situation**: A load balancer or monitoring system requests `/health` to verify the proxy is alive.

**What Happens**: The proxy recognizes this as an admin path and handles it internally. The health module checks the current health state (stored in an atomic boolean) and returns a JSON response. No rate limiting occurs, and the upstream is never contacted.

**Why It Matters**: Health checks must always succeed, even during a DDoS attack when rate limits would block normal traffic. This ensures Kubernetes, AWS ELB, or other orchestrators can accurately determine the proxy's status.

```
Client                              Proxy                           
   │                                  │                             
   │ ─── GET /health ───────────────► │                             
   │                                  │                             
   │                          [Check: is_admin_path("/health")]     
   │                          [Result: TRUE]                        
   │                          [Generate health JSON response]       
   │                                  │                             
   │ ◄── 200 OK ──────────────────────│                             
   │     {"status":"healthy",         │                             
   │      "version":"0.1.0"}          │                             
```

**Metrics Recorded**: None (admin endpoints don't count as traffic)

---

### Scenario 2: Normal Request Within Rate Limits

**Situation**: A regular API request arrives, no payment provided, and the client hasn't exceeded their rate limit.

**What Happens**: The proxy extracts the client's IP address (from X-Forwarded-For, X-Real-IP, or the connection itself), checks for payment headers (none found), and then consults the rate limiter. The rate limiter reports "Allowed" because neither the global nor per-IP limit has been exceeded. The proxy forwards the request to the upstream server and returns the response to the client.

**Why It Matters**: This is the "happy path" for free-tier users. As long as clients stay within limits, they experience no friction—the proxy is nearly invisible.

```
Client                              Proxy                           Upstream
   │                                  │                                │
   │ ─── GET /api/users ────────────► │                                │
   │                                  │                                │
   │                          [Extract IP: 192.168.1.100]              │
   │                          [Check payment header: NONE]             │
   │                          [Rate limit check: ALLOWED]              │
   │                                  │                                │
   │                                  │ ─── GET /api/users ──────────► │
   │                                  │                                │
   │                                  │ ◄── 200 OK ────────────────────│
   │                                  │     [user data]                │
   │                                  │                                │
   │ ◄── 200 OK ──────────────────────│                                │
   │     [user data]                  │                                │
```

**Metrics Recorded**: 
- `requests_total{status="allowed", method="GET"}` +1

---

### Scenario 3: Per-IP Rate Limit Exceeded

**Situation**: A single client (identified by IP address) makes too many requests in a short time, exceeding the per-IP limit (e.g., 11th request when limit is 10/second).

**What Happens**: The proxy detects the client IP has exceeded its allocation. Instead of returning a generic 429 (Too Many Requests), it returns **HTTP 402 (Payment Required)** with a special `PAYMENT-REQUIRED` header. This header contains Base64-encoded JSON explaining exactly how the client can pay to get access.

**Why It Matters**: This is the monetization moment. Legitimate users who need more access can pay; bots and scrapers cannot (easily). The 402 response includes all information needed for a wallet or payment client to construct a valid payment.

**The 402 Response Contains**:
- The payment scheme (currently "exact")
- The blockchain network (e.g., "base-sepolia")
- The payment amount (e.g., "0.001")
- The recipient wallet address
- The specific resource being accessed

```
Client (192.168.1.100)              Proxy                           
   │                                  │                             
   │ ─── GET /api/data ─────────────► │  (11th request in 1 second)
   │                                  │                             
   │                          [Extract IP: 192.168.1.100]           
   │                          [Check payment: NONE]                 
   │                          [Rate limit check: IP EXCEEDED]       
   │                          [Generate payment requirements]       
   │                                  │                             
   │ ◄── 402 Payment Required ────────│                             
   │     PAYMENT-REQUIRED: eyJzY2...  │                             
   │     {                            │                             
   │       "error": "payment_required"│                             
   │       "message": "Rate limit     │                             
   │         exceeded. Pay to access."│                             
   │       "paymentRequirements": {   │                             
   │         "scheme": "exact",       │                             
   │         "network": "base-sepolia"│                             
   │         "maxAmountRequired":"0.001"                            
   │         "payTo": "0xYourWallet", │                             
   │         "resource": "/api/data"  │                             
   │       }                          │                             
   │     }                            │                             
```

**Metrics Recorded**:
- `requests_total{status="rate_limited", method="GET"}` +1
- `rate_limited_total{layer="ip"}` +1

---

### Scenario 4: Global Rate Limit Exceeded (DDoS Protection)

**Situation**: The server is experiencing massive traffic from many different IPs (a DDoS attack or viral traffic spike). The total requests per second exceeds the global threshold.

**What Happens**: The proxy checks the global rate limit **first**, before even looking at individual IPs. When global limit is exceeded, ALL non-paying traffic is blocked with 402 responses. This is a "circuit breaker" that protects your upstream from being overwhelmed.

**Why It Matters**: During an attack, you don't want to spend CPU cycles checking thousands of individual IP limits. By checking global first, the proxy can reject traffic immediately. The only clients who get through are those with valid payment signatures.

**Key Difference from Per-IP Limiting**: In per-IP limiting, only the offending IP is blocked. In global limiting, everyone is blocked (unless they pay). This is intentional—when your backend is at capacity, you need to prioritize paid traffic.

```
Many IPs sending traffic simultaneously...

Client (any IP)                     Proxy                           
   │                                  │                             
   │ ─── GET /api/anything ─────────► │  (server under heavy load)
   │                                  │                             
   │                          [Global rate check: 5000 req/s]       
   │                          [Global limit: 1000 req/s]            
   │                          [Result: GLOBAL EXCEEDED]             
   │                          [Per-IP check: SKIPPED]               
   │                                  │                             
   │ ◄── 402 Payment Required ────────│                             
```

**Metrics Recorded**:
- `requests_total{status="rate_limited", method="GET"}` +1
- `rate_limited_total{layer="global"}` +1

---

### Scenario 5: Valid Payment Bypasses Rate Limits

**Situation**: A client previously received a 402 response, made a crypto payment through the x402 protocol, and now sends a request with the `PAYMENT-SIGNATURE` header.

**What Happens**: 

1. **Detection**: The proxy finds the `PAYMENT-SIGNATURE` header and decodes the Base64 content into a payment payload.

2. **Verification**: The proxy sends the payment to the x402 facilitator's `/verify` endpoint. The facilitator checks that the payment signature is valid, the amount is sufficient, and the payment hasn't been used before.

3. **Bypass**: If verification succeeds, the proxy marks this request as "paid" and **completely skips rate limiting**. The request goes straight to the upstream.

4. **Settlement**: After the upstream responds successfully, the proxy calls the facilitator's `/settle` endpoint to finalize the payment transaction on the blockchain.

5. **Confirmation**: The proxy adds a `PAYMENT-RESPONSE` header to the response, confirming the transaction hash.

**Why It Matters**: This is the core value proposition of x402. Legitimate users who need guaranteed access can pay for it. The proxy becomes a revenue source rather than just a cost center.

```
Client                  Proxy                     Facilitator    Upstream
   │                      │                            │            │
   │ ─ GET /api/data ───► │                            │            │
   │   PAYMENT-SIGNATURE: │                            │            │
   │   eyJzY2hlbWUi...    │                            │            │
   │                      │                            │            │
   │              [Decode payment signature]           │            │
   │                      │                            │            │
   │                      │ ─ POST /verify ──────────► │            │
   │                      │   (payment + requirements) │            │
   │                      │                            │            │
   │                      │ ◄─ {"isValid": true} ──────│            │
   │                      │                            │            │
   │              [Payment verified! Skip rate limits] │            │
   │                      │                            │            │
   │                      │ ─── GET /api/data ─────────────────────►│
   │                      │                            │            │
   │                      │ ◄── 200 OK ─────────────────────────────│
   │                      │                            │            │
   │              [Settle payment after success]       │            │
   │                      │                            │            │
   │                      │ ─ POST /settle ──────────► │            │
   │                      │                            │            │
   │                      │ ◄─ {"success": true, ──────│            │
   │                      │     "txHash": "0xabc..."}  │            │
   │                      │                            │            │
   │ ◄─ 200 OK ───────────│                            │            │
   │    PAYMENT-RESPONSE: │                            │            │
   │    eyJzdWNjZXNz...   │                            │            │
   │    [api data]        │                            │            │
```

**Metrics Recorded**:
- `requests_total{status="paid", method="GET"}` +1
- `payments_total{status="settled"}` +1

---

### Scenario 6: Invalid Payment Falls Back to Rate Limiting

**Situation**: A client provides a `PAYMENT-SIGNATURE` header, but the payment is invalid (wrong signature, expired, already used, insufficient amount, etc.).

**What Happens**: The proxy attempts verification, but the facilitator responds that the payment is invalid. The proxy logs a warning (for debugging) and then **falls through to normal rate limiting**. This prevents attackers from bypassing limits with fake payment signatures.

**Why It Matters**: The proxy doesn't trust unverified payments. If verification fails for any reason, the client is treated as a regular (non-paying) user. This ensures that only real payments provide benefits.

```
Client                  Proxy                     Facilitator    
   │                      │                            │         
   │ ─ GET /api/data ───► │                            │         
   │   PAYMENT-SIGNATURE: │                            │         
   │   (invalid sig)      │                            │         
   │                      │                            │         
   │                      │ ─ POST /verify ──────────► │         
   │                      │                            │         
   │                      │ ◄─ {"isValid": false, ─────│         
   │                      │     "reason": "expired"}   │         
   │                      │                            │         
   │              [Payment rejected - fall through]    │         
   │              [Now check rate limits...]           │         
   │                      │                            │         
   │              [If within limits → forward request] │         
   │              [If exceeded → return 402]           │         
```

**Metrics Recorded**:
- `payments_total{status="failed"}` +1
- Plus normal request/rate-limit metrics depending on outcome

---

### Scenario 7: Facilitator Unavailable (Graceful Degradation)

**Situation**: The external x402 facilitator service is down or unreachable. A client sends a request with a payment signature.

**What Happens**: The proxy attempts to call the facilitator but the HTTP request fails (timeout, connection refused, etc.). The proxy logs an error and treats the payment as unverified. The request falls through to normal rate limiting.

**Why It Matters**: The proxy continues to function even when the facilitator is unavailable. Free-tier traffic still flows (within limits). Only paid bypass is temporarily unavailable. This is **graceful degradation**—reduced functionality rather than complete failure.

```
Client                  Proxy                     Facilitator    
   │                      │                            X (down)    
   │ ─ GET /api/data ───► │                            │         
   │   PAYMENT-SIGNATURE: │                            │         
   │   (valid sig)        │                            │         
   │                      │                            │         
   │                      │ ─ POST /verify ──────────► X         
   │                      │   (connection timeout)     │         
   │                      │                            │         
   │              [Error: Facilitator unreachable]     │         
   │              [Log error, don't trust payment]     │         
   │              [Fall through to rate limiting]      │         
   │                      │                            │         
```

**Metrics Recorded**:
- `payments_total{status="failed"}` +1

---

## Component Integration

This section explains how data flows between components.

### Configuration Loading

At startup, `main.rs` reads the TOML configuration file and parses it into a strongly-typed `Config` struct. This config is then used to initialize all other components:

- **RateLimiter** receives `per_ip_requests_per_second`, `global_requests_per_second`, and `window_seconds`
- **X402Handler** receives `facilitator_url`, `recipient_address`, `amount`, `network`, `token`, and `description`
- **Pingora Server** receives `bind` address and optional TLS configuration

### Shared State

The `X402Proxy` struct holds all shared state wrapped in `Arc` (Atomic Reference Count) for thread-safe sharing across Pingora's worker threads:

- `config: Arc<Config>` - Immutable configuration
- `rate_limiter: RateLimiter` - Contains `Arc<RwLock<HashMap>>` internally
- `x402_handler: X402Handler` - Stateless, uses async HTTP client

### Metrics Collection

Throughout the request lifecycle, various components call into `metrics.rs` to record events:

- `record_request(status, method)` - Called in `proxy.rs` after determining request outcome
- `record_rate_limited(layer)` - Called when rate limit is exceeded
- `record_payment(status)` - Called after payment verification/settlement
- `set_tracked_ips(count)` - Called to update the gauge of tracked IPs

---

## Error Handling Strategy

The proxy is designed to fail safely. Here's how different errors are handled:

| Error | Handling | User Impact |
|-------|----------|-------------|
| Config file missing | Exit with error message | Proxy won't start |
| Config parse error | Exit with error message | Proxy won't start |
| Invalid payment signature | Log warning, fall to rate limit | Normal rate limiting |
| Facilitator unreachable | Log error, fall to rate limit | Payment bypass unavailable |
| Settlement failed | Log warning, response still sent | User gets data, no receipt |
| Upstream unreachable | Pingora returns 502 | User sees error |
| Upstream timeout | Pingora returns 504 | User sees error |

The key principle: **Never let a payment-related error prevent legitimate traffic from flowing**. If anything goes wrong with the payment system, the proxy falls back to acting as a normal rate-limiting reverse proxy.
