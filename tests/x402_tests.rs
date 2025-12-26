//! Unit tests for the x402 payment module

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use x402_reverseproxy::X402Handler;

#[test]
fn test_generate_payment_required_header() {
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient1234567890".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xTokenAddress".to_string(),
        "Test payment description".to_string(),
    );

    let header = handler.generate_payment_required_header("/api/test");

    // Should be valid base64
    let decoded = BASE64.decode(&header).expect("Should be valid base64");

    // Should be valid JSON
    let json: serde_json::Value = serde_json::from_slice(&decoded).expect("Should be valid JSON");

    // Verify fields
    assert_eq!(json["scheme"], "exact");
    assert_eq!(json["network"], "base-sepolia");
    assert_eq!(json["resource"], "/api/test");
    assert_eq!(json["payTo"], "0xRecipient1234567890");
    assert_eq!(json["maxAmountRequired"], "0.001");
}

#[test]
fn test_generate_402_body() {
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.01".to_string(),
        "base".to_string(),
        "0xToken".to_string(),
        "Pay to access API".to_string(),
    );

    let body = handler.generate_402_body("/protected/resource");

    // Should be valid JSON
    let json: serde_json::Value = serde_json::from_str(&body).expect("Should be valid JSON");

    assert_eq!(json["error"], "payment_required");
    assert!(json["message"].as_str().is_some());
    assert!(json["paymentRequirements"].is_object());

    let requirements = &json["paymentRequirements"];
    assert_eq!(requirements["resource"], "/protected/resource");
    assert_eq!(requirements["network"], "base");
}

#[test]
fn test_parse_payment_signature_valid() {
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xToken".to_string(),
        "Test".to_string(),
    );

    // Create a valid payment payload
    let payload = serde_json::json!({
        "scheme": "exact",
        "network": "base-sepolia",
        "payload": {
            "signature": "0x1234...",
            "authorization": {}
        }
    });

    let encoded = BASE64.encode(serde_json::to_string(&payload).unwrap());

    let result = handler.parse_payment_signature(&encoded);
    assert!(result.is_ok());

    let parsed = result.unwrap();
    assert_eq!(parsed.scheme, "exact");
    assert_eq!(parsed.network, "base-sepolia");
}

#[test]
fn test_parse_payment_signature_invalid_base64() {
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xToken".to_string(),
        "Test".to_string(),
    );

    let result = handler.parse_payment_signature("not-valid-base64!!!");
    assert!(result.is_err());
}

#[test]
fn test_parse_payment_signature_invalid_json() {
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xToken".to_string(),
        "Test".to_string(),
    );

    // Valid base64, but not JSON
    let encoded = BASE64.encode("this is not json");

    let result = handler.parse_payment_signature(&encoded);
    assert!(result.is_err());
}

#[test]
fn test_different_resources_different_headers() {
    let handler = X402Handler::new(
        "https://facilitator.example.com".to_string(),
        "0xRecipient".to_string(),
        "0.001".to_string(),
        "base-sepolia".to_string(),
        "0xToken".to_string(),
        "Test".to_string(),
    );

    let header1 = handler.generate_payment_required_header("/api/v1");
    let header2 = handler.generate_payment_required_header("/api/v2");

    // Headers should be different (different resources)
    assert_ne!(header1, header2);

    // Decode and verify
    let decoded1: serde_json::Value =
        serde_json::from_slice(&BASE64.decode(&header1).unwrap()).unwrap();
    let decoded2: serde_json::Value =
        serde_json::from_slice(&BASE64.decode(&header2).unwrap()).unwrap();

    assert_eq!(decoded1["resource"], "/api/v1");
    assert_eq!(decoded2["resource"], "/api/v2");
}
