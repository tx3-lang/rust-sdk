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
//! ## Module Overview
//!
//! - [`core`]: Core types and data structures used across the SDK
//! - [`tii`]: Transaction Invocation Interface for loading and interacting with TX3 protocols
//! - [`trp`]: Transaction Resolve Protocol client for submitting and tracking transactions
//!
//! ## Links
//!
//! - [TX3 Documentation](https://tx3.land)
//! - [GitHub Repository](https://github.com/tx3-lang/rust-sdk)
//! - [Crates.io](https://crates.io/crates/tx3-sdk)

pub mod core;
pub mod tii;
pub mod trp;
