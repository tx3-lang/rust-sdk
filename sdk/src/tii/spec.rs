use schemars::schema::Schema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Root structure for TII (Transaction Invocation Interface) JSON files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiiFile {
    pub tii: TiiInfo,
    pub protocol: Protocol,
    pub transactions: HashMap<String, Transaction>,
    #[serde(default = "HashMap::new")]
    pub environments: HashMap<String, Environment>,
    #[serde(default)]
    pub components: Components,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Transaction definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub tir: tx3_tir::interop::json::TirEnvelope,
    pub params: Schema,
}

/// Environment definition
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Environment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub defaults: Option<EnvironmentDefaults>,
}

/// Environment defaults
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentDefaults {
    pub schema: Schema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defaults: Option<serde_json::Value>,
}

/// Components section containing schemas and other components
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Components {
    #[serde(default = "HashMap::new")]
    pub schemas: HashMap<String, Schema>,
}
