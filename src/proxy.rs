//! Proxy module implementing Pingora's ProxyHttp trait.
//!
//! This is the core of the reverse proxy, handling request filtering,
//! rate limiting, x402 payment validation, and upstream forwarding.

use async_trait::async_trait;
use pingora_core::prelude::*;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::config::Config;
use crate::health;
use crate::metrics;
use crate::rate_limiter::{RateLimitResult, RateLimiter};
use crate::x402::{headers, X402Handler};

/// Context passed through the request lifecycle.
pub struct RequestContext {
    /// Client IP address
    pub client_ip: String,
    /// Parsed x402 payment (if provided)
    pub payment: Option<crate::x402::PaymentPayload>,
    /// Whether this request bypassed rate limits via payment
    pub paid: bool,
    /// Request path for payment requirements
    pub path: String,
}

/// x402 Reverse Proxy implementation.
pub struct X402Proxy {
    /// Configuration
    config: Arc<Config>,
    /// Rate limiter
    rate_limiter: RateLimiter,
    /// x402 payment handler
    x402_handler: X402Handler,
}

impl X402Proxy {
    /// Create a new x402 proxy instance.
    pub fn new(config: Config) -> Self {
        let rate_limiter = RateLimiter::new(
            config.rate_limits.per_ip_requests_per_second,
            config.rate_limits.global_requests_per_second,
            config.rate_limits.window_seconds,
        );

        let x402_handler = X402Handler::new(
            config.x402.facilitator_url.clone(),
            config.x402.recipient_address.clone(),
            config.x402.amount.clone(),
            config.x402.network.clone(),
            config.x402.token.clone(),
            config.x402.description.clone(),
        );

        Self {
            config: Arc::new(config),
            rate_limiter,
            x402_handler,
        }
    }

    /// Extract client IP from the session.
    fn get_client_ip(&self, session: &Session) -> String {
        // Try X-Forwarded-For header first
        if let Some(xff) = session.req_header().headers.get("X-Forwarded-For") {
            if let Ok(xff_str) = xff.to_str() {
                // Take the first IP in the chain
                if let Some(first_ip) = xff_str.split(',').next() {
                    return first_ip.trim().to_string();
                }
            }
        }

        // Fall back to X-Real-IP
        if let Some(real_ip) = session.req_header().headers.get("X-Real-IP") {
            if let Ok(ip_str) = real_ip.to_str() {
                return ip_str.to_string();
            }
        }

        // Fall back to peer address
        session
            .client_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Check for x402 payment signature and validate if present.
    async fn check_payment(&self, session: &Session, ctx: &mut RequestContext) -> bool {
        let payment_header = session.req_header().headers.get(headers::PAYMENT_SIGNATURE);

        if let Some(header_value) = payment_header {
            if let Ok(header_str) = header_value.to_str() {
                match self.x402_handler.parse_payment_signature(header_str) {
                    Ok(payment) => {
                        // Verify payment with facilitator
                        match self.x402_handler.verify_payment(&payment, &ctx.path).await {
                            Ok(true) => {
                                info!(client_ip = %ctx.client_ip, "Valid x402 payment - bypassing rate limits");
                                ctx.payment = Some(payment);
                                ctx.paid = true;
                                return true;
                            }
                            Ok(false) => {
                                warn!(client_ip = %ctx.client_ip, "Invalid x402 payment");
                            }
                            Err(e) => {
                                warn!(client_ip = %ctx.client_ip, error = %e, "Payment verification failed");
                            }
                        }
                    }
                    Err(e) => {
                        debug!(client_ip = %ctx.client_ip, error = %e, "Failed to parse payment signature");
                    }
                }
            }
        }

        false
    }

    /// Build and send a 402 Payment Required response.
    async fn build_402_response(
        &self,
        session: &mut Session,
        ctx: &RequestContext,
    ) -> pingora_core::Result<bool> {
        let mut resp = ResponseHeader::build(402, None)?;

        // Add x402 headers
        let payment_required = self
            .x402_handler
            .generate_payment_required_header(&ctx.path);
        resp.insert_header(headers::PAYMENT_REQUIRED, payment_required)?;
        resp.insert_header("Content-Type", "application/json")?;

        // Generate body
        let body = self.x402_handler.generate_402_body(&ctx.path);

        session.set_keepalive(None);
        session.write_response_header(Box::new(resp), false).await?;
        session.write_response_body(Some(body.into()), true).await?;

        Ok(true)
    }
}

#[async_trait]
impl ProxyHttp for X402Proxy {
    type CTX = RequestContext;

    fn new_ctx(&self) -> Self::CTX {
        RequestContext {
            client_ip: String::new(),
            payment: None,
            paid: false,
            path: String::new(),
        }
    }

    async fn request_filter(
        &self,
        session: &mut Session,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<bool> {
        // Extract client info
        ctx.client_ip = self.get_client_ip(session);
        ctx.path = session.req_header().uri.path().to_string();
        let method = session.req_header().method.as_str().to_string();

        debug!(client_ip = %ctx.client_ip, path = %ctx.path, method = %method, "Incoming request");

        // Step 0: Handle health/admin endpoints (bypass rate limiting)
        if health::is_admin_path(&ctx.path) {
            if let Some((status, body, content_type)) = health::handle_admin_request(&ctx.path) {
                let (resp, body_bytes) = health::build_admin_response(status, body, content_type)?;
                session.set_keepalive(None);
                session.write_response_header(Box::new(resp), false).await?;
                session
                    .write_response_body(Some(body_bytes.into()), true)
                    .await?;
                return Ok(true); // Response sent, don't forward to upstream
            }
        }

        // Step 1: Check for x402 payment - if valid, bypass all rate limits
        if self.check_payment(session, ctx).await {
            metrics::record_request("paid", &method);
            return Ok(false); // Continue to upstream
        }

        // Step 2: Check rate limits (dual-layer defense)
        // Update tracked IPs metric
        metrics::set_tracked_ips(self.rate_limiter.stats().tracked_ips);

        match self.rate_limiter.check(&ctx.client_ip) {
            RateLimitResult::Allowed => {
                debug!(client_ip = %ctx.client_ip, "Request allowed (within rate limits)");
                metrics::record_request("allowed", &method);
                Ok(false) // Continue to upstream
            }
            RateLimitResult::IpLimitExceeded => {
                warn!(client_ip = %ctx.client_ip, "Per-IP rate limit exceeded");
                metrics::record_request("rate_limited", &method);
                metrics::record_rate_limited("ip");
                self.build_402_response(session, ctx).await
            }
            RateLimitResult::GlobalLimitExceeded => {
                warn!(client_ip = %ctx.client_ip, "Global rate limit exceeded (DDoS protection)");
                metrics::record_request("rate_limited", &method);
                metrics::record_rate_limited("global");
                self.build_402_response(session, ctx).await
            }
        }
    }

    async fn upstream_peer(
        &self,
        _session: &mut Session,
        _ctx: &mut Self::CTX,
    ) -> pingora_core::Result<Box<HttpPeer>> {
        // Parse upstream URL
        let upstream_url = &self.config.upstream.url;

        // Simple parsing - extract host and port
        let (host, port, tls) = parse_upstream_url(upstream_url);

        let peer = HttpPeer::new((host.as_str(), port), tls, host.clone());
        Ok(Box::new(peer))
    }

    async fn response_filter(
        &self,
        _session: &mut Session,
        upstream_response: &mut ResponseHeader,
        ctx: &mut Self::CTX,
    ) -> pingora_core::Result<()> {
        // If this was a paid request, settle the payment and add response header
        if let Some(ref payment) = ctx.payment {
            match self.x402_handler.settle_payment(payment, &ctx.path).await {
                Ok(settle_response) => {
                    let payment_response = self
                        .x402_handler
                        .generate_payment_response_header(&settle_response);
                    upstream_response.insert_header(headers::PAYMENT_RESPONSE, payment_response)?;
                    info!(client_ip = %ctx.client_ip, "Payment settled successfully");
                }
                Err(e) => {
                    warn!(client_ip = %ctx.client_ip, error = %e, "Failed to settle payment");
                }
            }
        }

        Ok(())
    }
}

/// Parse upstream URL into host, port, and TLS flag.
fn parse_upstream_url(url: &str) -> (String, u16, bool) {
    let tls = url.starts_with("https://");
    let url_without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    // Remove path if present
    let host_port = url_without_scheme
        .split('/')
        .next()
        .unwrap_or(url_without_scheme);

    // Split host and port
    if let Some((host, port_str)) = host_port.rsplit_once(':') {
        let port = port_str.parse().unwrap_or(if tls { 443 } else { 80 });
        (host.to_string(), port, tls)
    } else {
        let port = if tls { 443 } else { 80 };
        (host_port.to_string(), port, tls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_upstream_url() {
        assert_eq!(
            parse_upstream_url("http://localhost:3000"),
            ("localhost".to_string(), 3000, false)
        );

        assert_eq!(
            parse_upstream_url("https://api.example.com"),
            ("api.example.com".to_string(), 443, true)
        );

        assert_eq!(
            parse_upstream_url("http://127.0.0.1:8080/api"),
            ("127.0.0.1".to_string(), 8080, false)
        );
    }
}
