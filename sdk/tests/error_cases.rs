//! Error case integration tests for the TRP (Transaction Resolution Protocol) client.
//!
//! These tests verify error handling and edge cases when interacting with a TRP server.
//!
//! # Running Tests
//!
//! To run these tests, you need to set the `TRP_ENDPOINT_PREPROD` environment variable:
//!
//! ```bash
//! TRP_ENDPOINT_PREPROD=https://trp.example.com cargo test --test error_cases
//! ```
//!
//! Optional environment variables:
//! - `TRP_API_KEY_PREPROD` - API key for authentication (if required by endpoint)
//!
//! If `TRP_ENDPOINT_PREPROD` is not set, tests will be skipped automatically.

use std::collections::HashMap;
use std::env;
use tx3_sdk::core::{TirEnvelope, TirEncoding};
use tx3_sdk::trp::{Client, ClientOptions, ResolveParams};

/// Gets the TRP endpoint from environment variable.
/// Returns None if not set, which will cause tests to skip.
fn get_trp_endpoint() -> Option<String> {
    env::var("TRP_ENDPOINT_PREPROD").ok()
}

/// Gets the TRP API key from environment variable.
/// Returns None if not set.
fn get_trp_api_key() -> Option<String> {
    env::var("TRP_API_KEY_PREPROD").ok()
}

/// Creates a TRP client using the endpoint from environment.
/// If TRP_API_KEY_PREPROD is set, it will be included as a header.
fn create_trp_client() -> Option<Client> {
    get_trp_endpoint().map(|endpoint| {
        let mut headers = HashMap::new();

        // Add TRP API key header if available
        if let Some(api_key) = get_trp_api_key() {
            headers.insert("dmtr-api-key".to_string(), api_key);
        }

        Client::new(ClientOptions {
            endpoint,
            headers: if headers.is_empty() { None } else { Some(headers) },
        })
    })
}

/// Test error handling with an invalid TIR envelope.
///
/// This test verifies that the TRP client properly handles error responses
/// from the server when given invalid input.
#[tokio::test]
async fn test_trp_resolve_invalid_tir() {
    let Some(client) = create_trp_client() else {
        println!("Skipping test_trp_resolve_invalid_tir: TRP_ENDPOINT_PREPROD not set");
        return;
    };

    let invalid_params = ResolveParams {
        tir: TirEnvelope {
            content: "invalid_cbor_data".to_string(),
            encoding: TirEncoding::Hex,
            version: "v1beta0".to_string(),
        },
        args: serde_json::Map::new(),
        env: None,
    };

    let result = client.resolve(invalid_params).await;

    match result {
        Ok(_) => {
            panic!("Invalid TIR was unexpectedly accepted - expected an error");
        }
        Err(e) => {
            println!("Invalid TIR correctly rejected: {}", e);
            let error_string = e.to_string();
            assert!(
                error_string.contains("InvalidTirBytes")
                    || error_string.contains("UnsupportedTir")
                    || error_string.contains("GenericRpcError")
                    || error_string.contains("invalid"),
                "Error should indicate invalid TIR: {}",
                error_string
            );
        }
    }
}
