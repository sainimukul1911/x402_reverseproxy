//! Health check and admin endpoints.
//!
//! Provides `/health`, `/ready`, and `/metrics` endpoints.

use pingora_http::ResponseHeader;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::metrics;

/// Global health state
static IS_HEALTHY: AtomicBool = AtomicBool::new(true);
static IS_READY: AtomicBool = AtomicBool::new(false);

/// Set the server as ready (call after initialization)
pub fn set_ready(ready: bool) {
    IS_READY.store(ready, Ordering::SeqCst);
}

/// Set the server health status
pub fn set_healthy(healthy: bool) {
    IS_HEALTHY.store(healthy, Ordering::SeqCst);
}

/// Check if the server is healthy
pub fn is_healthy() -> bool {
    IS_HEALTHY.load(Ordering::SeqCst)
}

/// Check if the server is ready
pub fn is_ready() -> bool {
    IS_READY.load(Ordering::SeqCst)
}

/// Health check response
#[derive(serde::Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// Readiness check response
#[derive(serde::Serialize)]
pub struct ReadyResponse {
    pub ready: bool,
    pub checks: ReadinessChecks,
}

#[derive(serde::Serialize)]
pub struct ReadinessChecks {
    pub rate_limiter: bool,
    pub upstream: bool,
}

/// Check if a path is a health/admin endpoint
pub fn is_admin_path(path: &str) -> bool {
    matches!(path, "/health" | "/ready" | "/metrics" | "/_health" | "/_ready" | "/_metrics")
}

/// Generate response for health endpoints
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
                    rate_limiter: true, // Always ready if server is running
                    upstream: ready,    // Set to true once upstream is verified
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

/// Build HTTP response for admin endpoints
pub fn build_admin_response(status: u16, body: String, content_type: &str) -> pingora_core::Result<(ResponseHeader, Vec<u8>)> {
    let mut resp = ResponseHeader::build(status, None)?;
    resp.insert_header("Content-Type", content_type)?;
    resp.insert_header("Content-Length", body.len().to_string())?;
    resp.insert_header("Cache-Control", "no-cache, no-store")?;
    Ok((resp, body.into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check() {
        set_healthy(true);
        assert!(is_healthy());
        
        let (status, body, _) = handle_admin_request("/health").unwrap();
        assert_eq!(status, 200);
        assert!(body.contains("healthy"));
    }

    #[test]
    fn test_is_admin_path() {
        assert!(is_admin_path("/health"));
        assert!(is_admin_path("/metrics"));
        assert!(is_admin_path("/_ready"));
        assert!(!is_admin_path("/api/users"));
    }
}
