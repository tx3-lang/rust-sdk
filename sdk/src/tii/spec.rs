use schemars::schema::Schema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::TirEnvelope;

/// Root structure for TII (Transaction Invocation Interface) JSON files.
///
/// This structure represents the complete contents of a TII file, which defines
/// a TX3 protocol including its transactions, parties, profiles, and configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiiFile {
    /// TII specification version information.
    pub tii: TiiInfo,

    /// Protocol metadata (name, version, description).
    pub protocol: Protocol,

    /// Optional JSON schema for environment parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<Schema>,

    /// Map of party names to their definitions.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parties: HashMap<String, Party>,

    /// Map of transaction names to their definitions.
    pub transactions: HashMap<String, Transaction>,

    /// Map of profile names to their environment configurations.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub profiles: HashMap<String, Profile>,

    /// Optional reusable components (schemas, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub components: Option<Components>,
}

/// TII version information.
///
/// Specifies the version of the TII specification used by this file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiiInfo {
    /// The TII specification version (e.g., "1.0.0").
    pub version: String,
}

/// Protocol metadata.
///
/// Contains descriptive information about the TX3 protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    /// The protocol name.
    pub name: String,

    /// The protocol version.
    pub version: String,

    /// The protocol scope (e.g., "mainnet", "public").
    #[serde(default)]
    pub scope: String,

    /// Optional protocol description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Transaction definition.
///
/// Defines a single transaction within a TX3 protocol, including its
/// intermediate representation and parameter schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// The Transaction Intermediate Representation envelope.
    pub tir: TirEnvelope,

    /// JSON schema defining the transaction parameters.
    pub params: Schema,

    /// Optional transaction description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Party definition.
///
/// Represents a participant in a TX3 protocol (e.g., sender, receiver).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Party {
    /// Optional party description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Environment profile definition.
///
/// Profiles allow pre-configuration of environment-specific values for different
/// networks or contexts (mainnet, preview, testnet, etc.).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Profile {
    /// Optional profile description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Environment variables as JSON object.
    #[serde(default)]
    pub environment: serde_json::Value,

    /// Party addresses for this profile.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub parties: HashMap<String, String>,
}

/// Components section containing reusable schemas.
///
/// This section defines reusable components that can be referenced
/// throughout the TII file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Components {
    /// Map of reusable JSON schemas.
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub schemas: HashMap<String, Schema>,
}
