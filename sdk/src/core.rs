use serde::{Deserialize, Serialize};

/// Flexible key-value arguments for transaction execution
pub type ArgMap = serde_json::Map<String, serde_json::Value>;

/// Environment variables for transaction execution context
pub type EnvMap = serde_json::Map<String, serde_json::Value>;

/// Bech32-encoded address
pub type Address = String;

/// UTXO reference in the format 0x[64hex]#[index]
pub type UtxoRef = String;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BytesEnvelope {
    #[serde(alias = "payload")]
    pub content: String,
    #[serde(rename = "contentType", alias = "encoding")]
    pub content_type: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum TirEncoding {
    Hex,
    Base64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TirEnvelope {
    pub content: String,
    pub encoding: TirEncoding,
    pub version: String,
}
