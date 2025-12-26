//! x402 Payment Protocol Module
//!
//! Implements the Coinbase x402 protocol for HTTP micropayments:
//! - PAYMENT-REQUIRED header generation (402 responses)
//! - PAYMENT-SIGNATURE header validation
//! - Facilitator integration for /verify and /settle

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors related to x402 payment processing.
#[derive(Error, Debug)]
pub enum X402Error {
    #[error("Invalid payment signature header")]
    InvalidSignature,
    
    #[error("Failed to decode base64: {0}")]
    Base64Decode(#[from] base64::DecodeError),
    
    #[error("Failed to parse JSON: {0}")]
    JsonParse(#[from] serde_json::Error),
    
    #[error("Facilitator request failed: {0}")]
    FacilitatorRequest(#[from] reqwest::Error),
    
    #[error("Payment verification failed: {0}")]
    VerificationFailed(String),
    
    #[error("Payment settlement failed: {0}")]
    SettlementFailed(String),
}

/// Payment requirements sent in PAYMENT-REQUIRED header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    /// Payment scheme (e.g., "exact")
    pub scheme: String,
    /// Blockchain network (e.g., "base-sepolia")
    pub network: String,
    /// Maximum amount in token units
    pub max_amount_required: String,
    /// Resource being accessed
    pub resource: String,
    /// Description for the client
    pub description: String,
    /// MIME type
    pub mime_type: String,
    /// Recipient address
    pub pay_to: String,
    /// Maximum timeout in seconds
    pub max_timeout_seconds: u64,
    /// Token contract address
    pub asset: String,
    /// Optional extra data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Payment payload received in PAYMENT-SIGNATURE header.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    /// The scheme used
    pub scheme: String,
    /// Network
    pub network: String,
    /// Payload data (scheme-specific)
    pub payload: serde_json::Value,
}

/// Verification request sent to facilitator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

/// Verification response from facilitator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
}

/// Settlement request sent to facilitator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleRequest {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

/// Settlement response from facilitator.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// x402 Payment handler for the proxy.
#[derive(Clone)]
pub struct X402Handler {
    /// HTTP client for facilitator requests
    client: reqwest::Client,
    /// Facilitator base URL
    facilitator_url: String,
    /// Default payment requirements
    default_requirements: PaymentRequirements,
}

impl X402Handler {
    /// Create a new x402 handler.
    pub fn new(
        facilitator_url: String,
        recipient_address: String,
        amount: String,
        network: String,
        token: String,
        description: String,
    ) -> Self {
        let default_requirements = PaymentRequirements {
            scheme: "exact".to_string(),
            network,
            max_amount_required: amount,
            resource: "/".to_string(),
            description,
            mime_type: "application/json".to_string(),
            pay_to: recipient_address,
            max_timeout_seconds: 60,
            asset: token,
            extra: None,
        };
        
        Self {
            client: reqwest::Client::new(),
            facilitator_url,
            default_requirements,
        }
    }
    
    /// Generate the PAYMENT-REQUIRED header value for a 402 response.
    pub fn generate_payment_required_header(&self, resource: &str) -> String {
        let mut requirements = self.default_requirements.clone();
        requirements.resource = resource.to_string();
        
        let json = serde_json::to_string(&requirements).unwrap_or_default();
        BASE64.encode(json.as_bytes())
    }
    
    /// Generate the full 402 response body.
    pub fn generate_402_body(&self, resource: &str) -> String {
        let mut requirements = self.default_requirements.clone();
        requirements.resource = resource.to_string();
        
        serde_json::json!({
            "error": "payment_required",
            "message": "Rate limit exceeded. Pay to continue.",
            "paymentRequirements": requirements
        }).to_string()
    }
    
    /// Parse a PAYMENT-SIGNATURE header value.
    pub fn parse_payment_signature(&self, header_value: &str) -> Result<PaymentPayload, X402Error> {
        let decoded = BASE64.decode(header_value)?;
        let payload: PaymentPayload = serde_json::from_slice(&decoded)?;
        Ok(payload)
    }
    
    /// Verify a payment with the facilitator.
    pub async fn verify_payment(
        &self,
        payment: &PaymentPayload,
        resource: &str,
    ) -> Result<bool, X402Error> {
        let mut requirements = self.default_requirements.clone();
        requirements.resource = resource.to_string();
        
        let request = VerifyRequest {
            payment_payload: payment.clone(),
            payment_requirements: requirements,
        };
        
        let url = format!("{}/verify", self.facilitator_url);
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;
        
        let verify_response: VerifyResponse = response.json().await?;
        
        if verify_response.is_valid {
            Ok(true)
        } else {
            Err(X402Error::VerificationFailed(
                verify_response.invalid_reason.unwrap_or_else(|| "Unknown reason".to_string())
            ))
        }
    }
    
    /// Settle a payment with the facilitator.
    pub async fn settle_payment(
        &self,
        payment: &PaymentPayload,
        resource: &str,
    ) -> Result<SettleResponse, X402Error> {
        let mut requirements = self.default_requirements.clone();
        requirements.resource = resource.to_string();
        
        let request = SettleRequest {
            payment_payload: payment.clone(),
            payment_requirements: requirements,
        };
        
        let url = format!("{}/settle", self.facilitator_url);
        let response = self.client
            .post(&url)
            .json(&request)
            .send()
            .await?;
        
        let settle_response: SettleResponse = response.json().await?;
        
        if settle_response.success {
            Ok(settle_response)
        } else {
            Err(X402Error::SettlementFailed(
                settle_response.error.unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }
    
    /// Generate the PAYMENT-RESPONSE header for successful payments.
    pub fn generate_payment_response_header(&self, settle_response: &SettleResponse) -> String {
        let json = serde_json::to_string(settle_response).unwrap_or_default();
        BASE64.encode(json.as_bytes())
    }
}

/// HTTP header names for x402 protocol.
pub mod headers {
    /// Header sent by server in 402 response with payment requirements
    pub const PAYMENT_REQUIRED: &str = "PAYMENT-REQUIRED";
    
    /// Header sent by client with payment signature/payload
    pub const PAYMENT_SIGNATURE: &str = "PAYMENT-SIGNATURE";
    
    /// Header sent by server after successful payment settlement
    pub const PAYMENT_RESPONSE: &str = "PAYMENT-RESPONSE";
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_generate_payment_required_header() {
        let handler = X402Handler::new(
            "https://example.com".to_string(),
            "0x1234".to_string(),
            "0.001".to_string(),
            "base-sepolia".to_string(),
            "0xtoken".to_string(),
            "Test payment".to_string(),
        );
        
        let header = handler.generate_payment_required_header("/api/test");
        
        // Should be valid base64
        let decoded = BASE64.decode(&header).unwrap();
        let json: PaymentRequirements = serde_json::from_slice(&decoded).unwrap();
        
        assert_eq!(json.resource, "/api/test");
        assert_eq!(json.pay_to, "0x1234");
    }
    
    #[test]
    fn test_generate_402_body() {
        let handler = X402Handler::new(
            "https://example.com".to_string(),
            "0x1234".to_string(),
            "0.001".to_string(),
            "base-sepolia".to_string(),
            "0xtoken".to_string(),
            "Test payment".to_string(),
        );
        
        let body = handler.generate_402_body("/api/test");
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        
        assert_eq!(parsed["error"], "payment_required");
        assert!(parsed["paymentRequirements"].is_object());
    }
}
