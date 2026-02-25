//! Core types and data structures for the Tx3 SDK.
//!
//! This module provides the fundamental types used throughout the SDK for representing
//! transaction data, addresses, and various envelope formats.

use serde::{Deserialize, Serialize};

/// Flexible key-value arguments for transaction execution.
///
/// This type represents the arguments passed to a transaction invocation.
/// Keys are parameter names and values are JSON values that can represent
/// various types (strings, numbers, booleans, arrays, objects).
///
/// # Example
///
/// ```ignore
/// use serde_json::json;
/// use tx3_sdk::core::ArgMap;
///
/// let mut args = ArgMap::new();
/// args.insert("sender".to_string(), json!("addr1abc..."));
/// args.insert("amount".to_string(), json!(1000000));
/// args.insert("allow_bypass".to_string(), json!(true));
/// ```
pub type ArgMap = serde_json::Map<String, serde_json::Value>;

/// Environment variables for transaction execution context.
///
/// Environment variables provide additional context for transaction execution,
/// such as network parameters, protocol constants, or other configuration values
/// that affect how transactions are resolved.
///
/// # Example
///
/// ```ignore
/// use serde_json::json;
/// use tx3_sdk::core::EnvMap;
///
/// let mut env = EnvMap::new();
/// env.insert("network_id".to_string(), json!(1)); // mainnet
/// env.insert("slot".to_string(), json!(50000000));
/// ```
pub type EnvMap = serde_json::Map<String, serde_json::Value>;

/// Bech32-encoded blockchain address.
///
/// This type alias represents blockchain addresses in their Bech32-encoded form,
/// which is the human-readable format commonly used in wallets and explorers.
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::core::Address;
///
/// let address: Address = "addr1q9y8r3q4z3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3q3".to_string();
/// ```
pub type Address = String;

/// UTXO reference in the format `0x[64hex]#[index]`.
///
/// This type alias represents a reference to an unspent transaction output (UTXO)
/// on a UTxO-based blockchain. The format consists of:
/// - A 64-character hexadecimal transaction hash (prefixed with `0x`)
/// - A `#` separator
/// - An output index number
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::core::UtxoRef;
///
/// let utxo_ref: UtxoRef = "0xabc123...def456#0".to_string();
/// ```
pub type UtxoRef = String;

/// A generic envelope for byte-encoded data with content type information.
///
/// This structure wraps binary data (typically encoded as hex or base64 strings)
/// along with metadata about the content type and encoding. It's commonly used
/// for transaction bytes, signatures, and other cryptographic data.
///
/// # Fields
///
/// * `content` - The encoded data as a string (typically hex or base64)
/// * `content_type` - MIME type or encoding identifier (e.g., "application/cbor", "hex")
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::core::BytesEnvelope;
///
/// let envelope = BytesEnvelope {
///     content: "a10081825820abc123...".to_string(),
///     content_type: "application/cbor".to_string(),
/// };
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BytesEnvelope {
    /// The encoded payload content as a string.
    ///
    /// This field accepts either the alias "payload" or the primary name "content"
    /// for backward compatibility with different serialization formats.
    #[serde(alias = "payload")]
    pub content: String,

    /// The content type or encoding of the payload.
    ///
    /// This field accepts either "contentType" or the alias "encoding" for
    /// backward compatibility with different serialization formats.
    #[serde(rename = "contentType", alias = "encoding")]
    pub content_type: String,
}

/// Encoding format for Transaction Intermediate Representation (TIR) data.
///
/// This enum specifies how TIR data is encoded when serialized.
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TirEncoding {
    /// Hexadecimal encoding (e.g., "abc123...")
    Hex,
    /// Base64 encoding (e.g., "q83v...")
    Base64,
}

/// An envelope containing Transaction Intermediate Representation (TIR) data.
///
/// TIR is the intermediate format used by TX3 to represent transactions in a
/// protocol-agnostic way before they are resolved to specific blockchain transactions.
/// This envelope wraps the TIR content with metadata about its encoding and version.
///
/// # Fields
///
/// * `content` - The encoded TIR data
/// * `encoding` - The encoding format used (hex or base64)
/// * `version` - The TIR specification version (e.g., "v1beta0")
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::core::{TirEnvelope, TirEncoding};
///
/// let envelope = TirEnvelope {
///     content: "a10081825820...".to_string(),
///     encoding: TirEncoding::Hex,
///     version: "v1beta0".to_string(),
/// };
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TirEnvelope {
    /// The encoded TIR content.
    pub content: String,

    /// The encoding format of the content.
    pub encoding: TirEncoding,

    /// The TIR specification version.
    pub version: String,
}
