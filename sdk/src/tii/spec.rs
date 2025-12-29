use schemars::schema::Schema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::TirEnvelope;

/// Root structure for TII (Transaction Invocation Interface) JSON files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiiFile {
    pub tii: TiiInfo,

    pub protocol: Protocol,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Schema>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parties: HashMap<String, Party>,

    pub transactions: HashMap<String, Transaction>,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub profiles: HashMap<String, Profile>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Components>,
}

/// TII version information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiiInfo {
    pub version: String,
}

/// Protocol metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    pub name: String,
    pub version: String,

    #[serde(default)]
    pub scope: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Transaction definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub tir: TirEnvelope,
    pub params: Schema,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Environment definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default)]
    pub environment: serde_json::Value,

    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parties: HashMap<String, String>,
}

/// Components section containing schemas and other components
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Components {
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub schemas: HashMap<String, Schema>,
}
