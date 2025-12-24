use serde::{Deserialize, Serialize};

pub type ArgMap = serde_json::Map<String, serde_json::Value>;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BytesEnvelope {
    pub content: String,
    pub encoding: BytesEncoding,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "lowercase")]
pub enum BytesEncoding {
    Base64,
    Hex,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TirEnvelope {
    pub content: String,
    pub encoding: BytesEncoding,
    pub version: String,
}
