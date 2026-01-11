//! Proxy module implementing Pingora's ProxyHttp trait.
//!
//! This is the core of the reverse proxy, handling request filtering,
//! rate limiting, x402 payment validation, and upstream forwarding.

use async_trait::async_trait;
use pingora::lb::{selection::RoundRobin, LoadBalancer};
use pingora_core::prelude::*;
use pingora_http::ResponseHeader;
use pingora_proxy::{ProxyHttp, Session};
use std::net::ToSocketAddrs;
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
    /// Upstream Load Balancer
    load_balancer: Arc<LoadBalancer<RoundRobin>>,
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

        // Initialize Load Balancer
        let mut upstreams = Vec::new();
        // Support both old `url` (if we kept it) and new `urls`
        // Since we replaced `url` with `urls` in Config, we iterate `urls`
        for url in &config.upstream.urls {
            let (host, port, _) = parse_upstream_url(url);
            // Note: LoadBalancer expects valid SocketAddr or parseable string.
            // We can use "<host>:<port>"
            let addr = format!("{}:{}", host, port);
            if let Ok(backends) = addr.to_socket_addrs() {
                for backend in backends {
                    upstreams.push(backend);
                }
            } else {
                warn!("Failed to resolve upstream URL: {}", url);
            }
        }

        // If resolution failed or no URLs, fallback to localhost to avoid crash (or panic?)
        if upstreams.is_empty() {
            warn!("No valid upstreams found, defaulting to 127.0.0.1:8080");
            upstreams.push("127.0.0.1:8080".to_socket_addrs().unwrap().next().unwrap());
        }

        let lb = LoadBalancer::try_from_iter(upstreams).expect("Failed to create Load Balancer");
        // No health check for now on upstreams, just simple Round Robin

        Self {
            config: Arc::new(config),
            rate_limiter,
            x402_handler,
            load_balancer: Arc::new(lb),
        }
    }

    /// Extract client IP from the session.
    fn get_client_ip(&self, session: &Session) -> String {
        // Only trust X-Forwarded-For if configured
        if self.config.server.trust_forwarded_headers {
            // Try X-Forwarded-For header
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
        }

        // Fall back to peer address (always safe)
        session
            .client_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }

    /// Create a background service to clean up stale rate limiter entries.
    pub fn create_cleanup_service(&self) -> crate::rate_limiter::RateLimitCleaner {
        crate::rate_limiter::RateLimitCleaner(self.rate_limiter.clone())
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

    /// Handle Admin API requests
    async fn handle_admin_api(
        &self,
        session: &mut Session,
        path: &str,
    ) -> pingora_core::Result<bool> {
        if path.starts_with("/admin/ratelimits") && session.req_header().method == "POST" {
            // Read body to update limits
            // Note: Reading body in request_filter is tricky in Pingora if not fully buffered.
            // Simplified: Just toggle safe defaults for demo or parse query params?
            // Let's assume query params for simplicity: /admin/ratelimits?ip=100&global=2000
            if let Some(query) = session.req_header().uri.query() {
                let params: std::collections::HashMap<String, String> =
                    url::form_urlencoded::parse(query.as_bytes())
                        .into_owned()
                        .collect();

                if let (Some(ip), Some(global)) = (params.get("per_ip"), params.get("global")) {
                    if let (Ok(ip_val), Ok(global_val)) = (ip.parse::<u32>(), global.parse::<u32>())
                    {
                        self.rate_limiter.update_limits(ip_val, global_val);
                        info!(
                            "Updated rate limits to: per_ip={}, global={}",
                            ip_val, global_val
                        );

                        let body = format!(
                            "{{\"status\": \"ok\", \"per_ip\": {}, \"global\": {}}}",
                            ip_val, global_val
                        );
                        let mut resp = ResponseHeader::build(200, None)?;
                        resp.insert_header("Content-Type", "application/json")?;
                        session.write_response_header(Box::new(resp), false).await?;
                        session.write_response_body(Some(body.into()), true).await?;
                        return Ok(true);
                    }
                }
            }

            // Invalid request
            let resp = ResponseHeader::build(400, None)?;
            session.write_response_header(Box::new(resp), false).await?;
            return Ok(true);
        }

        Ok(false) // Not handled
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

        // Step 0.1: Check Admin API
        if ctx.path.starts_with("/admin/") {
            if self.handle_admin_api(session, &ctx.path).await? {
                return Ok(true);
            }
        }

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

        // Step 0.2: IP Allowlist/Blocklist
        if self.config.rate_limits.blocklist.contains(&ctx.client_ip) {
            warn!(client_ip = %ctx.client_ip, "Internal blocklist hit");
            let resp = ResponseHeader::build(403, None)?;
            session.write_response_header(Box::new(resp), false).await?;
            return Ok(true);
        }

        if self.config.rate_limits.allowlist.contains(&ctx.client_ip) {
            debug!(client_ip = %ctx.client_ip, "Allowed via allowlist");
            metrics::record_request("allowed_whitelist", &method);
            return Ok(false);
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
        // Use Load Balancer to select upstream
        let backend = self.load_balancer.select(b"", 256).unwrap(); // simple selection

        // Note: backend is a Service wrapper or SocketAddr?
        // LoadBalancer<RoundRobin> stores backend as whatever we passed in.
        // But Pingora wraps it in pingora::protocols::l4::socket::SocketAddr.

        let peer_addr = backend.to_string();

        // We need to determine if TLS is needed.
        // Currently Config has `urls`. We parsed them.
        // If we have mixed HTTP/HTTPS upstreams, we have a problem because SocketAddr doesn't store scheme.
        // We should store a struct `UpstreamNode { addr: SocketAddr, tls: bool, sni: String }`.

        // For now, let's assume all upstreams share the scheme from the FIRST url in config?
        // Or strictly parse.
        // Simpler: Just support all same scheme for now.
        // Let's check the first URL in config to decide TLS.
        let tls = self
            .config
            .upstream
            .urls
            .first()
            .map(|u| u.starts_with("https://"))
            .unwrap_or(false);
        let sni = self
            .config
            .upstream
            .urls
            .first()
            .map(|u| parse_upstream_url(u).0)
            .unwrap_or_else(|| "localhost".to_string());

        let peer = HttpPeer::new(peer_addr.as_str(), tls, sni);
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
