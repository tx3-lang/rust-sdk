//! Happy path integration test for the TRP (Transaction Resolution Protocol) client.
//!
//! This test follows a single transfer transaction through its complete lifecycle
//! from resolution to finalization. It represents the ideal successful path.
//!
//! # Running Tests
//!
//! To run this test, you need to set the following environment variables:
//!
//! ```bash
//! TRP_ENDPOINT=https://trp.example.com \
//! TRP_TEST_SENDER=addr_test1... \
//! TRP_TEST_RECEIVER=addr_test1... \
//! TRP_TEST_PRIVATE_KEY=your_private_key_in_hex \
//! DMTR_API_KEY=your-api-key \
//! cargo test --test happy_path
//! ```
//!
//! Required environment variables:
//! - `TRP_ENDPOINT` - The TRP server URL
//! - `TRP_TEST_SENDER` - Sender address (must have UTXOs available)
//! - `TRP_TEST_RECEIVER` - Receiver address
//! - `TRP_TEST_PRIVATE_KEY` - Private key in hex format for signing (64 characters = 32 bytes)
//!
//! Optional environment variables:
//! - `DMTR_API_KEY` - API key for authentication (if required by endpoint)
//!
//! If `TRP_ENDPOINT` is not set, the test will be skipped automatically.

use std::collections::HashMap;
use std::env;
use std::time::Duration;
use tx3_sdk::core::BytesEnvelope;
use tx3_sdk::tii::Protocol;
use tx3_sdk::trp::{
    Client, ClientOptions, SubmitParams, TxWitness, WitnessType,
};

/// Gets required environment variable or panics with a helpful message.
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
fn get_test_config() -> Option<(String, String, String)> {
    let sender = get_required_env("TRP_TEST_SENDER")?;
    let receiver = get_required_env("TRP_TEST_RECEIVER")?;
    let private_key = get_required_env("TRP_TEST_PRIVATE_KEY")?;
    Some((sender, receiver, private_key))
}

/// Gets the DMTR API key from environment variable.
/// Returns None if not set.
fn get_dmtr_api_key() -> Option<String> {
    env::var("DMTR_API_KEY").ok()
}

/// Creates a TRP client using the endpoint from environment.
/// If DMTR_API_KEY is set, it will be included as a header.
fn create_trp_client() -> Option<Client> {
    let endpoint = get_required_env("TRP_ENDPOINT")?;
    let mut headers = HashMap::new();

    // Add DMTR API key header if available
    if let Some(api_key) = get_dmtr_api_key() {
        headers.insert("dmtr-api-key".to_string(), api_key);
    }

    Some(Client::new(ClientOptions {
        endpoint,
        headers: if headers.is_empty() { None } else { Some(headers) },
    }))
}

/// Loads the transfer.tii protocol file.
fn load_transfer_protocol() -> Protocol {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let tii_path = format!("{manifest_dir}/../examples/transfer.tii");
    Protocol::from_file(&tii_path).expect("Failed to load transfer.tii")
}

/// Signs transaction bytes using ed25519 private key.
fn sign_transaction(tx_cbor: &[u8], private_key_hex: &str) -> (Vec<u8>, Vec<u8>) {
    // Decode private key from hex
    let private_key_bytes = hex::decode(private_key_hex).expect("Invalid private key hex");
    assert_eq!(
        private_key_bytes.len(),
        32,
        "Private key must be 32 bytes (64 hex characters)"
    );

    // Convert to fixed-size array (required by SecretKey)
    let mut key_array = [0u8; 32];
    key_array.copy_from_slice(&private_key_bytes);

    // Create signing key from bytes using From trait
    let signing_key: pallas_crypto::key::ed25519::SecretKey = key_array.into();
    let public_key = signing_key.public_key();

    // Sign the transaction
    let signature = signing_key.sign(tx_cbor);

    (
        public_key.as_ref().to_vec(),
        signature.as_ref().to_vec(),
    )
}

/// Test the complete happy path lifecycle of a transfer transaction.
///
/// This test follows a strict lifecycle:
/// 1. **Resolve** (STRICT): Must succeed - resolves transaction from TII
/// 2. **Submit** (STRICT): Must succeed - signs and submits transaction
/// 3. **Pending** (LENIENT): Polls mempool, continues if tx not found (may have moved)
/// 4. **Inflight** (LENIENT): Polls inflight queue, continues if tx not found (may have moved)
/// 5. **Finalized** (STRICT): Must find tx in logs within 10 attempts (5s delay each = 50s max)
///
/// The transaction is expected to move quickly through the lifecycle. The only phase
/// where we strictly expect to find it is in the finalized phase (dump_logs).
///
/// # Required Environment Variables
///
/// - `TRP_ENDPOINT` - TRP server URL
/// - `TRP_TEST_SENDER` - Sender address with UTXOs
/// - `TRP_TEST_RECEIVER` - Receiver address
/// - `TRP_TEST_PRIVATE_KEY` - Private key in hex (64 characters = 32 bytes)
///
/// # Optional Environment Variables
///
/// - `DMTR_API_KEY` - API key for authentication
#[tokio::test]
async fn test_trp_happy_path_lifecycle() {
    // Check required configuration
    let Some(client) = create_trp_client() else {
        println!("Skipping test: TRP_ENDPOINT not set");
        return;
    };

    let Some((sender, receiver, private_key)) = get_test_config() else {
        println!("Skipping test: Missing required test configuration");
        println!("Required: TRP_TEST_SENDER, TRP_TEST_RECEIVER, TRP_TEST_PRIVATE_KEY");
        return;
    };

    println!("=== Transfer Transaction Lifecycle Test (Happy Path) ===");
    println!("Sender: {}", sender);
    println!("Receiver: {}", receiver);

    let protocol = load_transfer_protocol();

    // =========================================================================
    // PHASE 1: RESOLVE (STRICT - must succeed)
    // =========================================================================
    println!("\n[Phase 1: Resolve] Resolving transfer transaction...");

    let invocation = protocol
        .invoke("transfer", Some("preview"))
        .expect("Failed to invoke transfer transaction");

    let invocation = invocation
        .with_arg("sender", serde_json::json!(&sender))
        .with_arg("receiver", serde_json::json!(&receiver))
        .with_arg("quantity", serde_json::json!(1_000_000));

    let resolve_params = invocation
        .into_resolve_request()
        .expect("Failed to create resolve request");

    let tx_envelope = client
        .resolve(resolve_params)
        .await
        .expect("RESOLVE FAILED: Transaction resolution must succeed");

    let tx_hash = tx_envelope.hash.clone();
    let tx_cbor = hex::decode(&tx_envelope.tx).expect("Invalid CBOR hex");

    println!("✓ Resolved transaction successfully");
    println!("  Hash: {}", tx_hash);
    println!("  TX size: {} bytes", tx_cbor.len());

    // =========================================================================
    // PHASE 2: SIGN & SUBMIT (STRICT - must succeed)
    // =========================================================================
    println!("\n[Phase 2: Sign & Submit] Signing and submitting transaction...");

    let (public_key, signature) = sign_transaction(&tx_cbor, &private_key);

    let witness = TxWitness {
        key: BytesEnvelope {
            content: hex::encode(&public_key),
            content_type: "application/cbor".to_string(),
        },
        signature: BytesEnvelope {
            content: hex::encode(&signature),
            content_type: "application/cbor".to_string(),
        },
        witness_type: WitnessType::VKey,
    };

    let submit_params = SubmitParams {
        tx: BytesEnvelope {
            content: hex::encode(&tx_cbor),
            content_type: "application/cbor".to_string(),
        },
        witnesses: vec![witness],
    };

    let submit_response = client
        .submit(submit_params)
        .await
        .expect("SUBMIT FAILED: Transaction submission must succeed");

    assert_eq!(
        submit_response.hash, tx_hash,
        "SUBMIT FAILED: Response hash doesn't match resolved hash"
    );

    println!("✓ Submitted transaction successfully");
    println!("  Confirmed hash: {}", submit_response.hash);

    // =========================================================================
    // PHASE 3: PENDING (LENIENT - continue if not found)
    // =========================================================================
    println!("\n[Phase 3: Pending] Checking pending mempool...");

    let mut found_in_pending = false;
    for attempt in 1..=3 {
        match client.peek_pending(Some(100), Some(false)).await {
            Ok(response) => {
                if response.entries.iter().any(|t| t.hash == tx_hash) {
                    println!(
                        "✓ Found transaction in pending mempool (attempt {})",
                        attempt
                    );
                    found_in_pending = true;
                    break;
                } else {
                    println!(
                        "  Attempt {}: Not in pending ({} txs in mempool)",
                        attempt,
                        response.entries.len()
                    );
                }
            }
            Err(e) => {
                panic!(
                    "PENDING POLL FAILED: peek_pending returned error on attempt {}: {}",
                    attempt, e
                );
            }
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    if !found_in_pending {
        println!("  Transaction not found in pending (may have already moved to inflight)");
    }

    // =========================================================================
    // PHASE 4: INFLIGHT (LENIENT - continue if not found)
    // =========================================================================
    println!("\n[Phase 4: Inflight] Checking inflight transactions...");

    let mut found_in_inflight = false;
    let mut last_seen_confirmations = 0;

    for attempt in 1..=5 {
        match client.peek_inflight(Some(100), Some(false)).await {
            Ok(response) => {
                if let Some(tx) = response.entries.iter().find(|t| t.hash == tx_hash) {
                    println!(
                        "✓ Found in inflight (attempt {}): stage={:?}, confirmations={}",
                        attempt, tx.stage, tx.confirmations
                    );
                    found_in_inflight = true;
                    last_seen_confirmations = tx.confirmations;

                    // If already finalized, we can skip to logs phase
                    if matches!(tx.stage, tx3_sdk::trp::TxStage::Finalized) {
                        println!("  Transaction already finalized!");
                        break;
                    }
                } else {
                    println!(
                        "  Attempt {}: Not in inflight ({} txs tracked)",
                        attempt,
                        response.entries.len()
                    );
                }
            }
            Err(e) => {
                panic!(
                    "INFLIGHT POLL FAILED: peek_inflight returned error on attempt {}: {}",
                    attempt, e
                );
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    if !found_in_inflight {
        println!("  Transaction not found in inflight (may have completed quickly)");
    }

    // =========================================================================
    // PHASE 5: FINALIZED (STRICT - must find in logs within 10 attempts)
    // =========================================================================
    println!("\n[Phase 5: Finalized] Searching for transaction in logs...");
    println!("  Will poll dump_logs up to 10 times with 5s delays (max 50s)");

    let mut found_in_logs = false;
    let mut cursor: Option<u64> = None;

    for attempt in 1..=10 {
        match client.dump_logs(cursor, Some(100), Some(false)).await {
            Ok(response) => {
                if let Some(log) = response.entries.iter().find(|l| l.hash == tx_hash) {
                    println!(
                        "✓ FOUND IN LOGS (attempt {}): stage={:?}, confirmations={}",
                        attempt, log.stage, log.confirmations
                    );
                    if let Some(ref point) = log.confirmed_at {
                        println!(
                            "  Confirmed at slot {} (block {})",
                            point.slot, point.block_hash
                        );
                    }
                    found_in_logs = true;
                    break;
                }

                println!(
                    "  Attempt {}: Scanned {} log entries, transaction not found",
                    attempt, response.entries.len()
                );

                cursor = response.next_cursor;
                if cursor.is_none() {
                    println!("  Reached end of logs without finding transaction");
                    break;
                }
            }
            Err(e) => {
                panic!(
                    "LOGS POLL FAILED: dump_logs returned error on attempt {}: {}",
                    attempt, e
                );
            }
        }

        if attempt < 10 {
            println!("  Waiting 5 seconds before next attempt...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    assert!(
        found_in_logs,
        "FINALIZED CHECK FAILED: Transaction {} not found in logs after 10 attempts (50s max wait). \
        Expected to see the finalized transaction in dump_logs but it was never recorded.",
        tx_hash
    );

    println!("\n=== Test Complete ===");
    println!("✓ All phases completed successfully");
    println!("  Transaction hash: {}", tx_hash);
    println!("  Found in pending: {}", found_in_pending);
    println!("  Found in inflight: {}", found_in_inflight);
    println!("  Max confirmations seen: {}", last_seen_confirmations);
    println!("  Found in logs: ✓");
}
