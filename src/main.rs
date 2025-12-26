//! x402 Reverse Proxy - Main Entry Point
//!
//! A high-performance reverse proxy that protects APIs from AI scraper spam
//! using dual-layer rate limiting with x402 micropayment wall.
//!
//! Usage:
//!   x402-proxy --config config.toml

use clap::Parser;
use pingora_core::prelude::*;
use pingora_proxy::http_proxy_service;
use std::path::PathBuf;
use tracing::{error, info, warn};
use tracing_subscriber::{
    fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

use x402_reverseproxy::{health, metrics, Config, X402Proxy};

/// x402 Reverse Proxy - Surge Protector for APIs
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

/// Initialize logging based on config and CLI args
fn init_logging(config: &Config, args: &Args) {
    let level = if args.debug {
        "debug"
    } else {
        &config.server.logging.level
    };

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));

    let use_json = args.json || config.server.logging.format == "json";

    if use_json {
        // JSON structured logging for production
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_span_events(FmtSpan::CLOSE)
                    .with_current_span(true)
                    .with_target(true)
                    .with_file(true)
                    .with_line_number(true),
            )
            .init();
    } else {
        // Pretty logging for development
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().pretty())
            .init();
    }
}

fn main() {
    let args = Args::parse();

    // Load configuration first (needed for logging config)
    let config = match Config::from_file(&args.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("ERROR: Failed to load configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize logging
    init_logging(&config, &args);

    info!(
        version = env!("CARGO_PKG_VERSION"),
        config_path = ?args.config,
        "x402 Reverse Proxy starting"
    );

    // Initialize metrics
    metrics::init_metrics();
    info!("Prometheus metrics initialized");

    // Log configuration
    info!(
        bind = %config.server.bind,
        upstream = %config.upstream.url,
        per_ip_limit = config.rate_limits.per_ip_requests_per_second,
        global_limit = config.rate_limits.global_requests_per_second,
        network = %config.x402.network,
        tls_enabled = config.server.tls.is_some(),
        "Configuration loaded"
    );

    // Create Pingora server
    let mut server = Server::new(None).unwrap();
    server.bootstrap();

    // Create the x402 proxy service
    let proxy = X402Proxy::new(config.clone());

    // Parse bind address
    let bind_addr = config.server.bind.clone();

    // Create HTTP proxy service
    let mut proxy_service = http_proxy_service(&server.configuration, proxy);

    // Add listener (with or without TLS)
    if let Some(tls_config) = &config.server.tls {
        info!(
            cert = %tls_config.cert_path,
            key = %tls_config.key_path,
            "TLS enabled"
        );
        // For TLS, we use add_tls with cert and key paths
        let _ = proxy_service.add_tls(&bind_addr, &tls_config.cert_path, &tls_config.key_path);
    } else {
        warn!("TLS disabled - running in HTTP mode (not recommended for production)");
        proxy_service.add_tcp(&bind_addr);
    }

    server.add_service(proxy_service);

    // Mark server as ready
    health::set_ready(true);

    info!(
        bind = %bind_addr,
        "x402 Reverse Proxy listening"
    );
    info!("Health endpoints: /health, /ready, /metrics");
    info!("Ready to protect your API! 🛡️");

    // Run the server
    server.run_forever();
}
