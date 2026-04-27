//! # TX3 Rust SDK
//!
//! Ergonomic Rust SDK for interacting with TX3 protocols, TII files, and TRP servers.
//!
//! TX3 is a DSL and protocol suite for defining and executing UTxO-based transactions in a
//! declarative, type-safe way.
//!
//! ## Quick start
//!
//! ```ignore
//! use serde_json::json;
//! use tx3_sdk::trp::{Client, ClientOptions};
//! use tx3_sdk::{CardanoSigner, Party, PollConfig, Tx3Client};
//!
//! let signer = CardanoSigner::from_mnemonic(
//!     "addr_test1...",
//!     "word1 word2 ... word24",
//! )?;
//!
//! let protocol = tx3_sdk::tii::Protocol::from_file("./examples/transfer.tii")?;
//! let trp = Client::new(ClientOptions {
//!     endpoint: "https://trp.example.com".to_string(),
//!     headers: None,
//! });
//!
//! let tx3 = Tx3Client::new(protocol, trp)
//!     .with_profile("preprod")
//!     .with_party("sender", Party::signer(signer))
//!     .with_party("receiver", Party::address("addr_test1..."))
//!     .with_party("middleman", Party::address("addr_test1..."));
//!
//! let status = tx3
//!     .tx("transfer")
//!     .arg("quantity", json!(10_000_000))
//!     .resolve()
//!     .await?
//!     .sign()?
//!     .submit()
//!     .await?
//!     .wait_for_confirmed(PollConfig::default())
//!     .await?;
//!
//! println!("Confirmed at stage: {:?}", status.stage);
//! ```
//!
//! ## Links
//!
//! - [TX3 Documentation](https://docs.txpipe.io/tx3)

pub mod core;
pub mod facade;
pub mod tii;
pub mod trp;

pub use facade::signer::{CardanoSigner, Ed25519Signer};
pub use facade::{
    Error, Party, PollConfig, ResolvedTx, SignRequest, SignedTx, Signer, SubmittedTx, Tx3Client,
    TxBuilder, WitnessInfo,
};
