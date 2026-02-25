//! Transaction Resolve Protocol (TRP) data types and structures.
//!
//! This module defines the request/response types used in the Transaction Resolve Protocol (TRP),
//! a JSON-RPC based protocol for resolving, submitting, and tracking UTxO transactions.
//!
//! TRP provides a standardized interface for:
//! - Resolving transaction templates into concrete blockchain transactions
//! - Submitting signed transactions to the network
//! - Monitoring transaction status and lifecycle
//! - Querying pending and inflight transaction queues

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::{ArgMap, BytesEnvelope, EnvMap, TirEnvelope};

/// Parameters for submitting a signed transaction to the network.
///
/// This structure wraps a signed transaction along with its cryptographic witnesses
/// (signatures) for submission to the blockchain network via TRP.
///
/// # Fields
///
/// * `tx` - The signed transaction as a bytes envelope
/// * `witnesses` - Vector of transaction witnesses (signatures)
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::trp::{SubmitParams, TxWitness, WitnessType};
/// use tx3_sdk::core::BytesEnvelope;
///
/// let submit_params = SubmitParams {
///     tx: BytesEnvelope {
///         content: "84a40081825820...".to_string(),
///         content_type: "application/cbor".to_string(),
///     },
///     witnesses: vec![TxWitness {
///         key: BytesEnvelope { /* ... */ },
///         signature: BytesEnvelope { /* ... */ },
///         witness_type: WitnessType::VKey,
///     }],
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitParams {
    /// The signed transaction bytes.
    #[serde(rename = "tx")]
    pub tx: BytesEnvelope,

    /// Cryptographic witnesses (signatures) for the transaction.
    #[serde(rename = "witnesses")]
    pub witnesses: Vec<TxWitness>,
}

/// A resolved transaction envelope returned by the TRP resolver.
///
/// This structure contains a fully resolved UTxO transaction ready for signing
/// and submission. The hash is computed from the transaction body.
///
/// # Fields
///
/// * `hash` - The transaction hash (hex-encoded)
/// * `tx` - The CBOR-encoded transaction as a hex string
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxEnvelope {
    /// The transaction hash (hex-encoded, 64 characters).
    #[serde(rename = "hash")]
    pub hash: String,

    /// The CBOR-encoded transaction bytes as a hex string.
    #[serde(rename = "tx")]
    pub tx: String,
}

/// Response from a successful transaction submission.
///
/// After submitting a signed transaction, the TRP server returns this structure
/// containing the transaction hash, which can be used to track the transaction status.
///
/// # Fields
///
/// * `hash` - The submitted transaction hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitResponse {
    /// The transaction hash that was submitted.
    #[serde(rename = "hash")]
    pub hash: String,
}

/// A cryptographic witness (signature) for a transaction.
///
/// Witnesses provide the proof that a transaction has been authorized by the
/// holder of a private key. Each witness includes the public key, signature,
/// and the type of witness.
///
/// # Fields
///
/// * `key` - The public key bytes
/// * `signature` - The cryptographic signature
/// * `witness_type` - The type of witness (currently only VKey supported)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxWitness {
    /// The public key bytes.
    #[serde(rename = "key")]
    pub key: BytesEnvelope,

    /// The cryptographic signature.
    #[serde(rename = "signature")]
    pub signature: BytesEnvelope,

    /// The type of witness.
    #[serde(rename = "type")]
    pub witness_type: WitnessType,
}

/// Type of transaction witness.
///
/// Identifies the witness type for a transaction signature. Currently,
/// only VKey (verification key) witnesses are supported.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WitnessType {
    /// Verification key witness (standard Ed25519 signature).
    VKey,
}

/// A point on the blockchain identified by slot and block hash.
///
/// Chain points uniquely identify a specific block in a UTxO-based blockchain,
/// used for tracking transaction confirmation and chain state.
///
/// # Fields
///
/// * `slot` - The slot number of the block
/// * `block_hash` - The hash of the block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainPoint {
    /// The slot number.
    #[serde(rename = "slot")]
    pub slot: u64,

    /// The block hash (hex-encoded).
    #[serde(rename = "blockHash")]
    pub block_hash: String,
}

/// The lifecycle stage of a transaction.
///
/// Transactions progress through several stages from submission to finalization.
/// This enum represents all possible stages in the transaction lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TxStage {
    /// Transaction is pending and waiting to be processed.
    Pending,

    /// Transaction has been propagated to the network.
    Propagated,

    /// Transaction has been acknowledged by a node.
    Acknowledged,

    /// Transaction has been confirmed in a block.
    Confirmed,

    /// Transaction has reached finality (sufficient confirmations).
    Finalized,

    /// Transaction was dropped from the mempool.
    Dropped,

    /// Transaction was rolled back due to chain reorganization.
    RolledBack,

    /// Unknown or unrecognized stage.
    Unknown,
}

/// Status information for a transaction.
///
/// Contains detailed status information including the current lifecycle stage,
/// confirmation counts, and the chain point where the transaction was confirmed.
///
/// # Fields
///
/// * `stage` - Current lifecycle stage
/// * `confirmations` - Number of confirmations received
/// * `non_confirmations` - Number of non-confirmations (conflicting blocks)
/// * `confirmed_at` - Chain point where first confirmed (if confirmed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxStatus {
    /// Current lifecycle stage.
    #[serde(rename = "stage")]
    pub stage: TxStage,

    /// Number of block confirmations.
    #[serde(rename = "confirmations")]
    pub confirmations: u64,

    /// Number of blocks that didn't include the transaction.
    #[serde(rename = "nonConfirmations")]
    pub non_confirmations: u64,

    /// Chain point where the transaction was first confirmed.
    #[serde(rename = "confirmedAt", skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<ChainPoint>,
}

/// A map of transaction hashes to their statuses.
///
/// Returned by the `check_status` call to get status for multiple transactions.
pub type TxStatusMap = HashMap<String, TxStatus>;

/// Response from checking the status of transactions.
///
/// Contains the status map for all requested transactions.
///
/// # Fields
///
/// * `statuses` - Map of transaction hash to status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckStatusResponse {
    /// Map of transaction hashes to their statuses.
    #[serde(rename = "statuses")]
    pub statuses: TxStatusMap,
}

/// A log entry for a transaction.
///
/// Represents a single entry in the transaction log, tracking the transaction's
/// progress through various stages.
///
/// # Fields
///
/// * `hash` - Transaction hash
/// * `stage` - Current stage
/// * `payload` - Optional payload data
/// * `confirmations` - Number of confirmations
/// * `non_confirmations` - Number of non-confirmations
/// * `confirmed_at` - Chain point of first confirmation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxLog {
    /// Transaction hash.
    #[serde(rename = "hash")]
    pub hash: String,

    /// Current lifecycle stage.
    #[serde(rename = "stage")]
    pub stage: TxStage,

    /// Optional payload data (CBOR-encoded).
    #[serde(rename = "payload", skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,

    /// Number of confirmations.
    #[serde(rename = "confirmations")]
    pub confirmations: u64,

    /// Number of non-confirmations.
    #[serde(rename = "nonConfirmations")]
    pub non_confirmations: u64,

    /// Chain point of first confirmation.
    #[serde(rename = "confirmedAt", skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<ChainPoint>,
}

/// Response from dumping transaction logs.
///
/// Returns a paginated list of transaction log entries and an optional
/// cursor for retrieving the next page.
///
/// # Fields
///
/// * `entries` - Vector of log entries
/// * `next_cursor` - Cursor for next page (if more entries exist)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DumpLogsResponse {
    /// Transaction log entries.
    #[serde(rename = "entries")]
    pub entries: Vec<TxLog>,

    /// Cursor for pagination (to fetch next page).
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<u64>,
}

/// A pending transaction entry.
///
/// Represents a transaction that is pending in the mempool, waiting to be
/// included in a block.
///
/// # Fields
///
/// * `hash` - Transaction hash
/// * `payload` - Optional transaction payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingTx {
    /// Transaction hash.
    #[serde(rename = "hash")]
    pub hash: String,

    /// Optional CBOR-encoded transaction payload.
    #[serde(rename = "payload", skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

/// Response from peeking at pending transactions.
///
/// Returns pending transactions from the mempool.
///
/// # Fields
///
/// * `entries` - Vector of pending transactions
/// * `has_more` - Whether more pending transactions exist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeekPendingResponse {
    /// Pending transaction entries.
    #[serde(rename = "entries")]
    pub entries: Vec<PendingTx>,

    /// Whether more entries exist beyond this page.
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

/// An in-flight transaction entry.
///
/// Represents a transaction that has been submitted and is being tracked
/// through its lifecycle stages.
///
/// # Fields
///
/// * `hash` - Transaction hash
/// * `stage` - Current lifecycle stage
/// * `confirmations` - Number of confirmations
/// * `non_confirmations` - Number of non-confirmations
/// * `confirmed_at` - Chain point of first confirmation
/// * `payload` - Optional transaction payload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflightTx {
    /// Transaction hash.
    #[serde(rename = "hash")]
    pub hash: String,

    /// Current lifecycle stage.
    #[serde(rename = "stage")]
    pub stage: TxStage,

    /// Number of confirmations.
    #[serde(rename = "confirmations")]
    pub confirmations: u64,

    /// Number of non-confirmations.
    #[serde(rename = "nonConfirmations")]
    pub non_confirmations: u64,

    /// Chain point of first confirmation.
    #[serde(rename = "confirmedAt", skip_serializing_if = "Option::is_none")]
    pub confirmed_at: Option<ChainPoint>,

    /// Optional CBOR-encoded transaction payload.
    #[serde(rename = "payload", skip_serializing_if = "Option::is_none")]
    pub payload: Option<String>,
}

/// Response from peeking at in-flight transactions.
///
/// Returns in-flight transactions being tracked by the TRP server.
///
/// # Fields
///
/// * `entries` - Vector of in-flight transactions
/// * `has_more` - Whether more in-flight transactions exist
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeekInflightResponse {
    /// In-flight transaction entries.
    #[serde(rename = "entries")]
    pub entries: Vec<InflightTx>,

    /// Whether more entries exist beyond this page.
    #[serde(rename = "hasMore")]
    pub has_more: bool,
}

/// Parameters for resolving a transaction template into a concrete transaction.
///
/// This structure contains all the information needed to resolve a TIR-encoded transaction
/// template into a concrete UTxO transaction, including the template itself, arguments,
/// and optional environment variables.
///
/// # Fields
///
/// * `tir` - The Transaction Intermediate Representation envelope containing the template
/// * `args` - Arguments to populate the template parameters
/// * `env` - Optional environment variables for resolution context
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::trp::ResolveParams;
/// use tx3_sdk::core::TirEnvelope;
///
/// let params = ResolveParams {
///     tir: TirEnvelope {
///         content: "a10081825820...".to_string(),
///         encoding: tx3_sdk::core::TirEncoding::Hex,
///         version: "v1beta0".to_string(),
///     },
///     args: serde_json::Map::new(),
///     env: None,
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveParams {
    /// Arguments to populate the transaction template parameters.
    #[serde(rename = "args")]
    pub args: ArgMap,

    /// The TIR envelope containing the transaction template.
    #[serde(rename = "tir")]
    pub tir: TirEnvelope,

    /// Optional environment variables for transaction resolution.
    #[serde(rename = "env", skip_serializing_if = "Option::is_none")]
    pub env: Option<EnvMap>,
}

/// Diagnostic information about the search space for input resolution.
///
/// Provides details about the UTXO search space when an input cannot be resolved,
/// helping debug why a particular input query failed.
///
/// # Fields
///
/// * `by_address_count` - Number of UTXOs found by address
/// * `by_asset_class_count` - Number of UTXOs found by asset class
/// * `by_ref_count` - Number of UTXOs found by reference
/// * `matched` - List of matched UTXO references
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSpaceDiagnostic {
    /// Count of UTXOs found by address query.
    #[serde(rename = "byAddressCount", skip_serializing_if = "Option::is_none")]
    pub by_address_count: Option<i64>,

    /// Count of UTXOs found by asset class query.
    #[serde(rename = "byAssetClassCount", skip_serializing_if = "Option::is_none")]
    pub by_asset_class_count: Option<i64>,

    /// Count of UTXOs found by reference query.
    #[serde(rename = "byRefCount", skip_serializing_if = "Option::is_none")]
    pub by_ref_count: Option<i64>,

    /// List of matched UTXO references.
    #[serde(rename = "matched")]
    pub matched: Vec<String>,
}

/// Diagnostic information about an input query.
///
/// Contains the details of an input query that was attempted during
/// transaction resolution.
///
/// # Fields
///
/// * `address` - The address being queried (if any)
/// * `collateral` - Whether this is a collateral input
/// * `min_amount` - Minimum amount requirements
/// * `refs` - Specific UTXO references to include
/// * `support_many` - Whether multiple UTXOs are supported
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputQueryDiagnostic {
    /// The address being queried.
    #[serde(rename = "address", skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,

    /// Whether this is a collateral input.
    #[serde(rename = "collateral")]
    pub collateral: bool,

    /// Minimum amount requirements by asset.
    #[serde(rename = "minAmount")]
    pub min_amount: std::collections::HashMap<String, String>,

    /// Specific UTXO references to include.
    #[serde(rename = "refs")]
    pub refs: Vec<String>,

    /// Whether multiple UTXOs are supported.
    #[serde(rename = "supportMany")]
    pub support_many: bool,
}

/// Diagnostic for unsupported TIR version.
///
/// Returned when the provided TIR version is not supported by the TRP server.
///
/// # Fields
///
/// * `expected` - The expected TIR version
/// * `provided` - The version that was provided
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnsupportedTirDiagnostic {
    /// The expected TIR version.
    #[serde(rename = "expected")]
    pub expected: String,

    /// The TIR version that was provided.
    #[serde(rename = "provided")]
    pub provided: String,
}

/// Diagnostic for an unresolved input.
///
/// Provides detailed information about why a specific input could not be
/// resolved during transaction construction.
///
/// # Fields
///
/// * `name` - The name of the unresolved input
/// * `query` - The input query that was attempted
/// * `search_space` - Information about the search space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputNotResolvedDiagnostic {
    /// The name of the input.
    #[serde(rename = "name")]
    pub name: String,

    /// The input query details.
    #[serde(rename = "query")]
    pub query: InputQueryDiagnostic,

    /// Search space diagnostic information.
    #[serde(rename = "search_space")]
    pub search_space: SearchSpaceDiagnostic,
}

/// Diagnostic for a missing transaction argument.
///
/// Returned when a required argument is not provided for a transaction
/// template.
///
/// # Fields
///
/// * `key` - The name of the missing argument
/// * `arg_type` - The expected type of the argument
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissingTxArgDiagnostic {
    /// The name of the missing argument.
    #[serde(rename = "key")]
    pub key: String,

    /// The expected type of the argument.
    #[serde(rename = "type")]
    pub arg_type: String,
}

/// Diagnostic for transaction script failure.
///
/// Contains log output from failed transaction script execution.
///
/// # Fields
///
/// * `logs` - Script execution log messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxScriptFailureDiagnostic {
    /// Script execution log messages.
    #[serde(rename = "logs")]
    pub logs: Vec<String>,
}
