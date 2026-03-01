//! Happy path integration test for the TRP (Transaction Resolution Protocol) client.
//!
//! This test follows a single transfer transaction through its complete lifecycle
//! from resolution to finalization using the ergonomic facade.
//!
//! # Running Tests
//!
//! To run this test, you need to set the following environment variables:
//!
//! ```bash
//! TRP_ENDPOINT_PREPROD=https://trp.example.com \
//! TEST_PARTY_A_ADDRESS=addr_test1... \
//! TEST_PARTY_A_MNEMONIC="word1 word2 ... word24" \
//! TRP_API_KEY_PREPROD=your-api-key \
//! cargo test --test happy_path
//! ```
//!
//! Required environment variables:
//! - `TRP_ENDPOINT_PREPROD` - The TRP server URL
//! - `TEST_PARTY_A_ADDRESS` - Address used for sender/receiver/middleman (must have UTXOs available)
//! - `TEST_PARTY_A_MNEMONIC` - BIP39 mnemonic phrase (12-24 words)
//!
//! Optional environment variables:
//! - `TRP_API_KEY_PREPROD` - API key for authentication (if required by endpoint)
//!
//! If `TRP_ENDPOINT_PREPROD` is not set, the test will be skipped automatically.

use std::collections::HashMap;
use std::env;

use serde_json::json;
use tx3_sdk::tii::Protocol;
use tx3_sdk::trp::{Client, ClientOptions};
use tx3_sdk::{CardanoSigner, Party, PollConfig, Tx3Client};

/// Gets required environment variable or prints a helpful message.
fn get_required_env(var: &str) -> Option<String> {
    match env::var(var) {
        Ok(val) => Some(val),
        Err(_) => {
            println!("Required environment variable {} is not set", var);
            None
        }
    }
}

/// Gets required test configuration.
/// Returns None if any required variable is missing.
fn get_test_config() -> Option<(String, String)> {
    let party = get_required_env("TEST_PARTY_A_ADDRESS")?;
    let mnemonic = get_required_env("TEST_PARTY_A_MNEMONIC")?;
    Some((party, mnemonic))
}

/// Gets the DMTR API key from environment variable.
/// Returns None if not set.
fn get_trp_api_key() -> Option<String> {
    env::var("TRP_API_KEY_PREPROD").ok()
}

/// Creates a TRP client using the endpoint from environment.
/// If TRP_API_KEY_PREPROD is set, it will be included as a header.
fn create_trp_client() -> Option<Client> {
    let endpoint = get_required_env("TRP_ENDPOINT_PREPROD")?;
    let mut headers = HashMap::new();

    // Add TRP API key header if available
    if let Some(api_key) = get_trp_api_key() {
        headers.insert("dmtr-api-key".to_string(), api_key);
    }

    Some(Client::new(ClientOptions {
        endpoint,
        headers: if headers.is_empty() {
            None
        } else {
            Some(headers)
        },
    }))
}

/// Loads the transfer.tii protocol file.
fn load_transfer_protocol() -> Protocol {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let tii_path = format!("{manifest_dir}/../examples/transfer.tii");
    Protocol::from_file(&tii_path).expect("Failed to load transfer.tii")
}

/// Test the complete happy path lifecycle of a transfer transaction.
///
/// This test follows a strict lifecycle:
/// 1. **Resolve** (STRICT): Must succeed - resolves transaction from TII
/// 2. **Sign & Submit** (STRICT): Must succeed - signs and submits transaction
/// 3. **Finalized** (STRICT): Polls check-status until finalized or timeout
#[tokio::test]
async fn test_trp_happy_path_lifecycle() {
    // Check required configuration
    let Some(trp) = create_trp_client() else {
        println!("Skipping test: TRP_ENDPOINT_PREPROD not set");
        return;
    };

    let Some((party, mnemonic)) = get_test_config() else {
        println!("Skipping test: Missing required test configuration");
        println!("Required: TEST_PARTY_A_ADDRESS, TEST_PARTY_A_MNEMONIC");
        return;
    };

    let protocol = load_transfer_protocol();
    let signer =
        CardanoSigner::from_mnemonic(&party, &mnemonic).expect("Invalid mnemonic or address");

    let tx3 = Tx3Client::new(protocol, trp.clone())
        .with_profile("preprod")
        .with_party("sender", Party::signer(signer))
        .with_party("middleman", Party::address(&party))
        .with_party("receiver", Party::address(&party));

    let resolved = tx3
        .tx("transfer")
        .arg("quantity", json!(10_000_000))
        .resolve()
        .await
        .expect("RESOLVE FAILED: Transaction resolution must succeed");

    let signed = resolved
        .sign()
        .expect("SIGN FAILED: Transaction signing must succeed");

    let submitted = signed
        .submit()
        .await
        .expect("SUBMIT FAILED: Transaction submission must succeed");

    let poll_config = PollConfig::default();

    let _status = match submitted.wait_for_confirmed(poll_config).await {
        Ok(status) => status,
        Err(err) => {
            let _ = trp.check_status(vec![submitted.hash.clone()]).await;
            panic!("CONFIRMED CHECK FAILED: Transaction did not confirm in time: {err}");
        }
    };
}
