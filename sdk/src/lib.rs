//! # Tx3 SDK for Rust
//!
//! This SDK provides tools for interacting with TX3 services and protocols on UTxO-based blockchains.
//!
//! ## What is TX3?
//!
//! TX3 is a domain-specific language (DSL) and protocol suite for defining and executing UTxO-based blockchain
//! transactions in a declarative, type-safe manner. It abstracts the complexity of UTXO management
//! and transaction construction while maintaining flexibility for complex DeFi and smart contract interactions.
//!
//! ## Key Features
//!
//! - **Transaction Invocation Interface (TII)**: Load and interact with TX3 protocol definitions
//! - **Transaction Resolve Protocol (TRP)**: Submit and track transactions through a standardized RPC interface
//! - **Type-Safe Parameters**: Schema-based parameter validation and serialization
//! - **Multi-Profile Support**: Environment-specific configurations for different networks (mainnet, preview, etc.)
//!
//! ## Quick Start
//!
//! ### Loading a Protocol
//!
//! ```ignore
//! use tx3_sdk::tii::Protocol;
//!
//! // Load a TX3 protocol definition from a TII file
//! let protocol = Protocol::from_file("path/to/protocol.tii")?;
//!
//! // Get available transactions
//! for (name, tx) in protocol.txs() {
//!     println!("Transaction: {}", name);
//! }
//! ```
//!
//! ### Invoking a Transaction
//!
//! ```ignore
//! use serde_json::json;
//! use tx3_sdk::tii::Protocol;
//!
//! let protocol = Protocol::from_file("protocol.tii")?;
//!
//! // Start an invocation with optional profile
//! let invocation = protocol.invoke("transfer", Some("preview"))?;
//!
//! // Set arguments
//! let invocation = invocation
//!     .with_arg("sender", json!("addr1..."))
//!     .with_arg("receiver", json!("addr1..."))
//!     .with_arg("amount", json!(1000000));
//!
//! // Check for unspecified required parameters
//! for (name, param_type) in invocation.unspecified_params() {
//!     println!("Missing parameter: {} (type: {:?})", name, param_type);
//! }
//! ```
//!
//! ### Using the TRP Client
//!
//! ```ignore
//! use tx3_sdk::trp::{Client, ClientOptions, ResolveParams};
//!
//! // Create a TRP client
//! let client = Client::new(ClientOptions {
//!     endpoint: "https://trp.example.com".to_string(),
//!     headers: None,
//! });
//!
//! // Resolve a transaction
//! let params = ResolveParams {
//!     tir: invocation.tir,
//!     args: invocation.args,
//!     env: None,
//! };
//!
//! let tx_envelope = client.resolve(params).await?;
//! println!("Resolved transaction hash: {}", tx_envelope.hash);
//!
//! // Submit a signed transaction
//! let submit_response = client.submit(SubmitParams {
//!     tx: signed_tx,
//!     witnesses: vec![witness],
//! }).await?;
//!
//! // Check transaction status
//! let status = client.check_status(vec![submit_response.hash]).await?;
//! ```
//!
//! ### Full Lifecycle (Facade)
//!
//! ```ignore
//! use serde_json::json;
//! use tx3_sdk::{Party, PollConfig, Tx3Client};
//! use tx3_sdk::CardanoSigner;
//!
//! # async fn demo(protocol: tx3_sdk::tii::Protocol, trp: tx3_sdk::trp::Client) -> Result<(), tx3_sdk::Error> {
//! let signer = CardanoSigner::from_hex("your_private_key_in_hex", "addr1...")?;
//! let tx3 = Tx3Client::new(protocol, trp)
//!     .with_profile("preprod")
//!     .with_party("sender", Party::signer("addr1...", signer))
//!     .with_party("receiver", Party::address("addr1..."));
//!
//! let status = tx3
//!     .tx("transfer")
//!     .arg("quantity", 10_000_000)
//!     .resolve()
//!     .await?
//!     .sign()?
//!     .submit()
//!     .await?
//!     .wait_for_confirmed(PollConfig::default())
//!     .await?;
//!
//! println!("Confirmed at stage: {:?}", status.stage);
//! # Ok(())
//! # }
//!
//! // Inspect witness payloads (what gets sent in submit)
//! # async fn inspect(protocol: tx3_sdk::tii::Protocol, trp: tx3_sdk::trp::Client) -> Result<(), tx3_sdk::Error> {
//! let tx3 = Tx3Client::new(protocol, trp)
//!     .with_profile("preprod")
//!     .with_party("sender", Party::signer("addr1...", CardanoSigner::from_hex("key", "addr1...")?));
//! let signed = tx3
//!     .tx("transfer")
//!     .arg("quantity", 10_000_000)
//!     .resolve()
//!     .await?
//!     .sign()?;
//! for info in signed.witnesses() {
//!     println!("party={} address={} key={} sig={} hash={}",
//!         info.party,
//!         info.address,
//!         info.key.content,
//!         info.signature.content,
//!         info.signed_hash,
//!     );
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Module Overview
//!
//! - [`core`]: Core types and data structures used across the SDK
//! - [`tii`]: Transaction Invocation Interface for loading and interacting with TX3 protocols
//! - [`trp`]: Transaction Resolve Protocol client for submitting and tracking transactions
//! - [`facade`]: Ergonomic lifecycle facade for TX3 transactions
//!
//! ## Links
//!
//! - [TX3 Documentation](https://tx3.land)
//! - [GitHub Repository](https://github.com/tx3-lang/rust-sdk)
//! - [Crates.io](https://crates.io/crates/tx3-sdk)

pub mod core;
pub mod tii;
pub mod trp;
pub mod facade;

pub use facade::{
    Error, Party, PollConfig, ResolvedTx, SignedTx, Signer, SubmittedTx, Tx3Client, TxBuilder,
    WitnessInfo,
};
pub use facade::signer::{CardanoSigner, Ed25519Signer};
