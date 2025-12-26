//! Unit tests for the configuration module

use x402_reverseproxy::Config;

#[test]
fn test_parse_minimal_config() {
    let toml_str = r#"
        [server]
        bind = "0.0.0.0:8080"
        
        [upstream]
        url = "http://localhost:3000"
        
        [rate_limits]
        per_ip_requests_per_second = 10
        global_requests_per_second = 1000
        
        [x402]
        facilitator_url = "https://x402.org/facilitator"
        recipient_address = "0x1234567890abcdef"
        amount = "0.001"
        network = "base-sepolia"
        token = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
    "#;

    let config: Config = toml::from_str(toml_str).unwrap();

    assert_eq!(config.server.bind, "0.0.0.0:8080");
    assert_eq!(config.upstream.url, "http://localhost:3000");
    assert_eq!(config.rate_limits.per_ip_requests_per_second, 10);
    assert_eq!(config.rate_limits.global_requests_per_second, 1000);
    assert_eq!(config.x402.network, "base-sepolia");

    // Check defaults
    assert_eq!(config.upstream.timeout_seconds, 30);
    assert_eq!(config.rate_limits.window_seconds, 1);
    assert!(config.server.tls.is_none());
}

#[test]
fn test_parse_full_config() {
    let toml_str = r#"
        [server]
        bind = "0.0.0.0:443"
        workers = 8
        
        [server.tls]
        cert_path = "/path/to/cert.pem"
        key_path = "/path/to/key.pem"
        
        [server.logging]
        format = "json"
        level = "debug"
        
        [upstream]
        url = "https://api.example.com"
        timeout_seconds = 60
        
        [rate_limits]
        per_ip_requests_per_second = 100
        global_requests_per_second = 10000
        window_seconds = 5
        
        [x402]
        facilitator_url = "https://custom.facilitator.com"
        recipient_address = "0xabcdef1234567890"
        amount = "0.01"
        network = "base"
        token = "0xTokenAddress"
        description = "Custom description"
    "#;

    let config: Config = toml::from_str(toml_str).unwrap();

    assert_eq!(config.server.workers, Some(8));
    assert!(config.server.tls.is_some());

    let tls = config.server.tls.as_ref().unwrap();
    assert_eq!(tls.cert_path, "/path/to/cert.pem");
    assert_eq!(tls.key_path, "/path/to/key.pem");

    assert_eq!(config.server.logging.format, "json");
    assert_eq!(config.server.logging.level, "debug");

    assert_eq!(config.upstream.timeout_seconds, 60);
    assert_eq!(config.rate_limits.window_seconds, 5);
    assert_eq!(config.x402.description, "Custom description");
}

#[test]
fn test_parse_invalid_config_missing_required() {
    let toml_str = r#"
        [server]
        bind = "0.0.0.0:8080"
        
        [upstream]
        url = "http://localhost:3000"
        
        # Missing rate_limits and x402 sections
    "#;

    let result: Result<Config, _> = toml::from_str(toml_str);
    assert!(result.is_err());
}

#[test]
fn test_default_logging_config() {
    let toml_str = r#"
        [server]
        bind = "0.0.0.0:8080"
        
        [upstream]
        url = "http://localhost:3000"
        
        [rate_limits]
        per_ip_requests_per_second = 10
        global_requests_per_second = 1000
        
        [x402]
        facilitator_url = "https://x402.org/facilitator"
        recipient_address = "0x1234"
        amount = "0.001"
        network = "base-sepolia"
        token = "0xtoken"
    "#;

    let config: Config = toml::from_str(toml_str).unwrap();

    // Default logging values
    assert_eq!(config.server.logging.format, "pretty");
    assert_eq!(config.server.logging.level, "info");
}
