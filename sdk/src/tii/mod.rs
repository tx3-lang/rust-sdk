//! Transaction Invocation Interface (TII) for loading and interacting with TX3 protocols.
//!
//! This module provides tools for loading TX3 protocol definitions from TII files and
//! invoking transactions with type-safe parameter handling.
//!
//! ## Overview
//!
//! The Transaction Invocation Interface (TII) is the bridge between TX3 protocol definitions
//! and concrete transaction execution. A TII file (typically with `.tii` extension) is a JSON
//! file that contains:
//!
//! - Protocol metadata (name, version, scope)
//! - Transaction definitions with their TIR (Transaction Intermediate Representation)
//! - Parameter schemas for each transaction
//! - Party definitions
//! - Environment profiles for different networks (mainnet, preview, etc.)
//!
//! ## Usage
//!
//! ### Loading a Protocol
//!
//! ```ignore
//! use tx3_sdk::tii::Protocol;
//!
//! // Load from a file
//! let protocol = Protocol::from_file("path/to/protocol.tii")?;
//!
//! // Or load from a string
//! let protocol = Protocol::from_string(tii_json)?;
//!
//! // Or load from JSON value
//! let protocol = Protocol::from_json(json_value)?;
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
//! // Invoke with an optional profile
//! let invocation = protocol.invoke("transfer", Some("preview"))?;
//!
//! // Set arguments using the builder pattern
//! let invocation = invocation
//!     .with_arg("sender", json!("addr1..."))
//!     .with_arg("receiver", json!("addr1..."))
//!     .with_arg("amount", json!(1000000));
//!
//! // Check for unspecified required parameters
//! for (name, param_type) in invocation.unspecified_params() {
//!     println!("Missing: {} (type: {:?})", name, param_type);
//! }
//!
//! // Convert to TRP resolve request
//! let resolve_params = invocation.into_resolve_request()?;
//! ```
//!
//! ## Profiles
//!
//! Profiles allow you to pre-configure environment-specific values (addresses, constants, etc.)
//! for different networks. When invoking a transaction with a profile, those values are
//! automatically populated.

use schemars::schema::{InstanceType, Schema, SingleOrVec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

use crate::{
    core::{ArgMap, TirEnvelope},
    tii::spec::{Profile, Transaction},
};

pub mod spec;

/// Error type for TII operations.
///
/// This enum represents all possible errors that can occur when loading
/// and interacting with TX3 protocol definitions.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid JSON in the TII file.
    #[error("invalid TII JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    /// Failed to read the TII file from disk.
    #[error("failed to read file: {0}")]
    IoError(#[from] std::io::Error),

    /// Transaction name not found in the protocol.
    #[error("unknown tx: {0}")]
    UnknownTx(String),

    /// Profile name not found in the protocol.
    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    /// Invalid JSON schema for transaction parameters.
    #[error("invalid params schema")]
    InvalidParamsSchema,

    /// Invalid parameter type encountered in schema.
    #[error("invalid param type")]
    InvalidParamType,
}

fn params_from_schema(schema: Schema) -> Result<ParamMap, Error> {
    let mut params = ParamMap::new();

    let as_object = schema.into_object();

    if let Some(obj_validation) = as_object.object {
        for (key, value) in obj_validation.properties {
            params.insert(key, ParamType::from_json_schema(value)?);
        }
    }

    Ok(params)
}

/// A TX3 protocol loaded from a TII file.
///
/// This structure represents a loaded TX3 protocol definition and provides
/// methods for inspecting transactions and creating invocations.
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::tii::Protocol;
///
/// let protocol = Protocol::from_file("protocol.tii")?;
///
/// // List all available transactions
/// for (name, tx) in protocol.txs() {
///     println!("Transaction: {}", name);
/// }
///
/// // Invoke a specific transaction
/// let invocation = protocol.invoke("transfer", Some("mainnet"))?;
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    spec: spec::TiiFile,
}

impl Protocol {
    /// Creates a Protocol from a JSON value.
    ///
    /// # Arguments
    ///
    /// * `json` - A `serde_json::Value` containing the TII file content
    ///
    /// # Returns
    ///
    /// Returns a `Protocol` on success, or an error if the JSON is invalid.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::tii::Protocol;
    /// use serde_json::json;
    ///
    /// let json = json!({
    ///     "tii": { "version": "1.0.0" },
    ///     "protocol": { "name": "MyProtocol", "version": "1.0.0" },
    ///     "transactions": {}
    /// });
    ///
    /// let protocol = Protocol::from_json(json)?;
    /// ```
    pub fn from_json(json: serde_json::Value) -> Result<Protocol, Error> {
        let spec = serde_json::from_value(json)?;

        Ok(Protocol { spec })
    }

    /// Creates a Protocol from a JSON string.
    ///
    /// # Arguments
    ///
    /// * `code` - A string containing the TII JSON content
    ///
    /// # Returns
    ///
    /// Returns a `Protocol` on success, or an error if the JSON is invalid.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::tii::Protocol;
    ///
    /// let tii_content = r#"{
    ///     "tii": { "version": "1.0.0" },
    ///     "protocol": { "name": "MyProtocol", "version": "1.0.0" },
    ///     "transactions": {}
    /// }"#;
    ///
    /// let protocol = Protocol::from_string(tii_content.to_string())?;
    /// ```
    pub fn from_string(code: String) -> Result<Protocol, Error> {
        let json = serde_json::from_str(&code)?;
        Self::from_json(json)
    }

    /// Creates a Protocol from a file path.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the TII file
    ///
    /// # Returns
    ///
    /// Returns a `Protocol` on success, or an error if the file cannot be read
    /// or the JSON is invalid.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::tii::Protocol;
    ///
    /// let protocol = Protocol::from_file("./my_protocol.tii")?;
    /// ```
    pub fn from_file(path: impl AsRef<std::path::Path>) -> Result<Protocol, Error> {
        let code = std::fs::read_to_string(path)?;
        Self::from_string(code)
    }

    fn ensure_tx(&self, key: &str) -> Result<&Transaction, Error> {
        let tx = self.spec.transactions.get(key);
        let tx = tx.ok_or(Error::UnknownTx(key.to_string()))?;

        Ok(tx)
    }

    fn ensure_profile(&self, key: &str) -> Result<&Profile, Error> {
        let env = self
            .spec
            .profiles
            .get(key)
            .ok_or_else(|| Error::UnknownProfile(key.to_string()))?;

        Ok(env)
    }

    /// Creates an invocation for a transaction.
    ///
    /// This method initializes an invocation for the specified transaction,
    /// optionally applying a profile to pre-populate arguments.
    ///
    /// # Arguments
    ///
    /// * `tx` - The name of the transaction to invoke
    /// * `profile` - Optional profile name to apply (e.g., "mainnet", "preview")
    ///
    /// # Returns
    ///
    /// Returns an `Invocation` that can be configured with arguments and
    /// converted to a TRP resolve request.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The transaction name is not found
    /// - The profile name is not found (if specified)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::tii::Protocol;
    ///
    /// let protocol = Protocol::from_file("protocol.tii")?;
    ///
    /// // Invoke with a profile
    /// let invocation = protocol.invoke("transfer", Some("mainnet"))?;
    ///
    /// // Invoke without a profile
    /// let invocation = protocol.invoke("transfer", None)?;
    /// ```
    pub fn invoke(&self, tx: &str, profile: Option<&str>) -> Result<Invocation, Error> {
        let tx = self.ensure_tx(tx)?;

        let profile = profile.map(|x| self.ensure_profile(x)).transpose()?;

        let mut out = Invocation {
            tir: tx.tir.clone(),
            params: ParamMap::new(),
            args: ArgMap::new(),
        };

        for party in self.spec.parties.keys() {
            out.params.insert(party.to_lowercase(), ParamType::Address);
        }

        if let Some(env) = &self.spec.environment {
            out.params.extend(params_from_schema(env.clone())?);
        }

        out.params.extend(params_from_schema(tx.params.clone())?);

        if let Some(profile) = profile {
            if let Some(env) = profile.environment.as_object() {
                let values = env.clone();
                out.set_args(values);
            }

            for (key, value) in profile.parties.iter() {
                out.set_arg(key, json!(value));
            }
        }

        Ok(out)
    }

    /// Returns all transactions defined in the protocol.
    ///
    /// # Returns
    ///
    /// Returns a reference to the map of transaction names to their definitions.
    pub fn txs(&self) -> &HashMap<String, spec::Transaction> {
        &self.spec.transactions
    }

    /// Returns all parties defined in the protocol.
    ///
    /// # Returns
    ///
    /// Returns a reference to the map of party names to their definitions.
    pub fn parties(&self) -> &HashMap<String, spec::Party> {
        &self.spec.parties
    }
}

/// Type of a transaction parameter.
///
/// This enum represents the various types that transaction parameters can have,
/// including primitives, complex types, and references to TX3 core types.
#[derive(Debug, Clone)]
pub enum ParamType {
    /// Byte array type (hex-encoded).
    Bytes,
    /// Integer type (signed or unsigned).
    Integer,
    /// Boolean type.
    Boolean,
    /// UTXO reference in format `0x[64hex]#[index]`.
    UtxoRef,
    /// Bech32-encoded blockchain address.
    Address,
    /// List of another parameter type.
    List(Box<ParamType>),
    /// Custom JSON schema type.
    Custom(Schema),
}

impl ParamType {
    fn from_json_type(instance_type: InstanceType) -> Result<ParamType, Error> {
        match instance_type {
            InstanceType::Integer => Ok(ParamType::Integer),
            InstanceType::Boolean => Ok(ParamType::Boolean),
            _ => Err(Error::InvalidParamType),
        }
    }

    /// Creates a parameter type from a JSON schema.
    ///
    /// This method interprets a JSON schema and converts it to the appropriate
    /// `ParamType`. It handles TX3 core type references (Bytes, Address, UtxoRef)
    /// as well as primitive types.
    ///
    /// # Arguments
    ///
    /// * `schema` - The JSON schema to convert
    ///
    /// # Returns
    ///
    /// Returns the corresponding `ParamType` on success.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema cannot be mapped to a known parameter type.
    pub fn from_json_schema(schema: Schema) -> Result<ParamType, Error> {
        let as_object = schema.into_object();

        if let Some(reference) = &as_object.reference {
            return match reference.as_str() {
                "https://tx3.land/specs/v1beta0/core#Bytes" => Ok(ParamType::Bytes),
                "https://tx3.land/specs/v1beta0/core#Address" => Ok(ParamType::Address),
                "https://tx3.land/specs/v1beta0/core#UtxoRef" => Ok(ParamType::UtxoRef),
                _ => Err(Error::InvalidParamType),
            };
        }

        if let Some(inner) = as_object.instance_type {
            return match inner {
                SingleOrVec::Single(x) => Self::from_json_type(*x),
                SingleOrVec::Vec(_) => Err(Error::InvalidParamType),
            };
        }

        Err(Error::InvalidParamType)
    }
}

/// Input query specification.
///
/// This type is currently a placeholder for future input query functionality.
pub struct InputQuery {}

/// Map of parameter names to their types.
///
/// Used to represent the complete set of parameters required for a transaction.
pub type ParamMap = HashMap<String, ParamType>;

/// Map of input queries.
///
/// Used to represent input queries for transaction resolution.
pub type QueryMap = BTreeMap<String, InputQuery>;

/// An active transaction invocation.
///
/// This structure represents a transaction that is being prepared for execution.
/// It holds the transaction template (TIR), parameter definitions, and current
/// argument values.
///
/// Use the builder methods (`with_arg`, `with_args`) to populate arguments,
/// then convert to a TRP resolve request using `into_resolve_request`.
///
/// # Example
///
/// ```ignore
/// use serde_json::json;
/// use tx3_sdk::tii::Protocol;
///
/// let protocol = Protocol::from_file("protocol.tii")?;
/// let invocation = protocol.invoke("transfer", None)?;
///
/// // Set arguments
/// let invocation = invocation
///     .with_arg("sender", json!("addr1..."))
///     .with_arg("amount", json!(1000000));
///
/// // Check what's missing
/// for (name, ty) in invocation.unspecified_params() {
///     println!("Need: {} ({:?})", name, ty);
/// }
///
/// // Convert to resolve request
/// let resolve_params = invocation.into_resolve_request()?;
/// ```
#[derive(Debug, Clone)]
pub struct Invocation {
    tir: TirEnvelope,
    params: ParamMap,
    args: ArgMap,
    // TODO: support explicit input specification
    // input_override: HashMap<String, v1beta0::UtxoSet>,

    // TODO: support explicit fee specification
    // fee_override: Option<u64>,
}

impl Invocation {
    /// Returns a reference to all parameters for this invocation.
    ///
    /// # Returns
    ///
    /// A reference to the map of parameter names to their types.
    pub fn params(&mut self) -> &ParamMap {
        &self.params
    }

    /// Returns an iterator over parameters that haven't been specified yet.
    ///
    /// This is useful for checking which required arguments are still missing
    /// before submitting the transaction.
    ///
    /// # Returns
    ///
    /// An iterator over (name, type) pairs for unspecified parameters.
    pub fn unspecified_params(&mut self) -> impl Iterator<Item = (&String, &ParamType)> {
        self.params
            .iter()
            .filter(|(k, _)| !self.args.contains_key(k.as_str()))
    }

    /// Sets a single argument value.
    ///
    /// # Arguments
    ///
    /// * `name` - The parameter name (case-insensitive)
    /// * `value` - The JSON value to set
    pub fn set_arg(&mut self, name: &str, value: serde_json::Value) {
        self.args.insert(name.to_lowercase().to_string(), value);
    }

    /// Sets multiple argument values at once.
    ///
    /// # Arguments
    ///
    /// * `args` - A map of argument names to values
    pub fn set_args(&mut self, args: ArgMap) {
        self.args.extend(args);
    }

    /// Sets a single argument value (builder pattern).
    ///
    /// This is the builder-pattern variant of `set_arg`, allowing chained calls.
    ///
    /// # Arguments
    ///
    /// * `name` - The parameter name (case-insensitive)
    /// * `value` - The JSON value to set
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining.
    pub fn with_arg(mut self, name: &str, value: serde_json::Value) -> Self {
        self.args.insert(name.to_lowercase().to_string(), value);
        self
    }

    /// Sets multiple argument values at once (builder pattern).
    ///
    /// This is the builder-pattern variant of `set_args`, allowing chained calls.
    ///
    /// # Arguments
    ///
    /// * `args` - A map of argument names to values
    ///
    /// # Returns
    ///
    /// Returns `self` for method chaining.
    pub fn with_args(mut self, args: ArgMap) -> Self {
        self.args.extend(args);
        self
    }

    /// Converts this invocation into a TRP resolve request.
    ///
    /// This method consumes the invocation and creates the parameters needed
    /// to call the TRP `resolve` method.
    ///
    /// # Returns
    ///
    /// Returns `ResolveParams` that can be passed to `trp::Client::resolve`.
    ///
    /// # Errors
    ///
    /// Currently this method always succeeds, but returns `Result` for future
    /// compatibility.
    pub fn into_resolve_request(self) -> Result<crate::trp::ResolveParams, Error> {
        let args = self.args.clone().into_iter().collect();

        let tir = self.tir.clone();

        Ok(crate::trp::ResolveParams {
            tir,
            args,
            // We're already merging env into params / args, no need to send it independently.
            // Having both mechanism is a footgun. We should revisit either the TRP schema to
            // remove the option or split how we send the env in the SDK.
            env: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use serde_json::json;

    use super::*;

    #[test]
    fn happy_path_smoke_test() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let tii = format!("{manifest_dir}/tests/fixtures/transfer.tii");

        let protocol = Protocol::from_file(&tii).unwrap();

        let invoke = protocol.invoke("transfer", Some("preprod")).unwrap();

        let mut invoke = invoke
            .with_arg("sender", json!("addr1abc"))
            .with_arg("quantity", json!(100_000_000));

        let all_params: HashSet<_> = invoke.params().keys().collect();

        assert_eq!(all_params.len(), 5);
        assert!(all_params.contains(&"sender".to_string()));
        assert!(all_params.contains(&"middleman".to_string()));
        assert!(all_params.contains(&"receiver".to_string()));
        assert!(all_params.contains(&"tax".to_string()));
        assert!(all_params.contains(&"quantity".to_string()));

        let unspecified_params: HashSet<_> = invoke.unspecified_params().map(|(k, _)| k).collect();

        assert_eq!(unspecified_params.len(), 2);
        assert!(unspecified_params.contains(&"middleman".to_string()));
        assert!(unspecified_params.contains(&"receiver".to_string()));

        let tx = invoke.into_resolve_request().unwrap();

        dbg!(&tx);
    }
}
