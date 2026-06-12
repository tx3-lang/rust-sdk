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

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
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

/// Builds a parameter-type map from a JSON schema's `properties`. Never fails:
/// unrecognized property schemas yield [`ParamType::Unknown`]. `components` is the
/// TII's `components.schemas` table, used to resolve `#/components/schemas/<Name>`
/// refs to user-defined record / variant types.
fn params_from_schema(schema: &Value, components: &HashMap<String, Value>) -> ParamMap {
    let mut params = ParamMap::new();

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, value) in properties {
            params.insert(key.clone(), ParamType::from_json_schema(value, components));
        }
    }

    params
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

        let components: HashMap<String, Value> = self
            .spec
            .components
            .as_ref()
            .map(|c| c.schemas.clone())
            .unwrap_or_default();

        for party in self.spec.parties.keys() {
            out.params.insert(party.to_lowercase(), ParamType::Address);
        }

        if let Some(env) = &self.spec.environment {
            out.params.extend(params_from_schema(env, &components));
        }

        out.params.extend(params_from_schema(&tx.params, &components));

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

    /// Returns all profiles defined in the protocol.
    pub fn profiles(&self) -> &HashMap<String, spec::Profile> {
        &self.spec.profiles
    }

    /// Starts a [`Tx3ClientBuilder`] for this protocol. Configure TRP options,
    /// optional profile selection, party bindings, and env overrides, then
    /// call `build()` to obtain a [`crate::Tx3Client`].
    pub fn client(self) -> crate::facade::Tx3ClientBuilder {
        crate::facade::Tx3ClientBuilder::from_protocol(self)
    }
}

/// Type of a transaction parameter.
///
/// This enum represents the various types that transaction parameters can have,
/// including primitives, compound types, and references to TX3 core types. It is
/// built from the TII params JSON schema by [`ParamType::from_json_schema`], which
/// never fails — any shape it does not recognize becomes [`ParamType::Unknown`].
#[derive(Debug, Clone)]
pub enum ParamType {
    /// Byte array type (hex-encoded).
    Bytes,
    /// Integer type (signed or unsigned).
    Integer,
    /// Boolean type.
    Boolean,
    /// Unit type (`{ "type": "null" }`).
    Unit,
    /// UTXO reference in format `0x[64hex]#[index]`.
    UtxoRef,
    /// Bech32-encoded blockchain address.
    Address,
    /// A resolved UTxO object.
    Utxo,
    /// An asset identified at runtime by policy and name.
    AnyAsset,
    /// Homogeneous, variable-length sequence (`array` + `items`).
    List(Box<ParamType>),
    /// Fixed-length, positionally-typed sequence (`array` + `prefixItems`).
    Tuple(Vec<ParamType>),
    /// String-keyed homogeneous map (`object` + `additionalProperties`).
    Map(Box<ParamType>),
    /// User-defined record (`object` + `properties`), field name → type.
    Record(BTreeMap<String, ParamType>),
    /// User-defined tagged union (`oneOf`), externally tagged.
    Variant(Vec<VariantCase>),
    /// A schema shape that could not be interpreted; carries the raw schema.
    Unknown(Value),
}

/// One case of a [`ParamType::Variant`].
#[derive(Debug, Clone)]
pub struct VariantCase {
    /// The case tag (the single `required` key of the externally-tagged object).
    pub tag: String,
    /// The case payload (typically a [`ParamType::Record`]).
    pub fields: Box<ParamType>,
}

impl ParamType {
    /// Maps a built-in core `$ref` to its kind by trailing name, so both the
    /// canonical `…/tii#/$defs/<Name>` and legacy `…/core#<Name>` forms resolve.
    fn core_ref_type(reference: &str) -> Option<ParamType> {
        let name = reference.rsplit(['#', '/']).next().unwrap_or("");
        match name {
            "Bytes" => Some(ParamType::Bytes),
            "Address" => Some(ParamType::Address),
            "UtxoRef" => Some(ParamType::UtxoRef),
            "Utxo" => Some(ParamType::Utxo),
            "AnyAsset" => Some(ParamType::AnyAsset),
            _ => None,
        }
    }

    /// Interprets one externally-tagged `oneOf` branch into a [`VariantCase`].
    fn variant_case(case: &Value, components: &HashMap<String, Value>) -> VariantCase {
        let tag = case
            .get("required")
            .and_then(Value::as_array)
            .and_then(|r| r.first())
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        let fields = case
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|props| props.get(&tag))
            .map(|fields| Self::from_json_schema(fields, components))
            .unwrap_or_else(|| ParamType::Unknown(case.clone()));

        VariantCase {
            tag,
            fields: Box::new(fields),
        }
    }

    /// Creates a parameter type from a JSON schema node.
    ///
    /// Interprets every shape `tx3c` can emit (see the SDK spec's
    /// `api-surface/args.md`). It never fails: an unrecognized shape — including a
    /// bare `string`, an unresolved object, or an unknown `$ref` — becomes
    /// [`ParamType::Unknown`] carrying the raw schema.
    ///
    /// # Arguments
    ///
    /// * `schema` - The JSON schema node to interpret
    /// * `components` - The TII's `components.schemas` table, used to resolve
    ///   `#/components/schemas/<Name>` references to user-defined types
    pub fn from_json_schema(schema: &Value, components: &HashMap<String, Value>) -> ParamType {
        let Some(obj) = schema.as_object() else {
            return ParamType::Unknown(schema.clone());
        };

        if let Some(reference) = obj.get("$ref").and_then(Value::as_str) {
            if let Some(name) = reference.strip_prefix("#/components/schemas/") {
                return match components.get(name) {
                    Some(resolved) => Self::from_json_schema(resolved, components),
                    None => ParamType::Unknown(schema.clone()),
                };
            }
            return Self::core_ref_type(reference)
                .unwrap_or_else(|| ParamType::Unknown(schema.clone()));
        }

        if let Some(cases) = obj.get("oneOf").and_then(Value::as_array) {
            return ParamType::Variant(
                cases
                    .iter()
                    .map(|case| Self::variant_case(case, components))
                    .collect(),
            );
        }

        match obj.get("type").and_then(Value::as_str) {
            Some("integer") => ParamType::Integer,
            Some("boolean") => ParamType::Boolean,
            Some("null") => ParamType::Unit,
            Some("array") => {
                if let Some(prefix) = obj.get("prefixItems").and_then(Value::as_array) {
                    ParamType::Tuple(
                        prefix
                            .iter()
                            .map(|el| Self::from_json_schema(el, components))
                            .collect(),
                    )
                } else if let Some(items) = obj.get("items").filter(|i| i.is_object()) {
                    ParamType::List(Box::new(Self::from_json_schema(items, components)))
                } else {
                    ParamType::Unknown(schema.clone())
                }
            }
            Some("object") => {
                if let Some(value) = obj.get("additionalProperties").filter(|v| v.is_object()) {
                    ParamType::Map(Box::new(Self::from_json_schema(value, components)))
                } else if let Some(props) = obj.get("properties").and_then(Value::as_object) {
                    ParamType::Record(
                        props
                            .iter()
                            .map(|(k, v)| (k.clone(), Self::from_json_schema(v, components)))
                            .collect(),
                    )
                } else {
                    ParamType::Unknown(schema.clone())
                }
            }
            _ => ParamType::Unknown(schema.clone()),
        }
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

    fn pt(schema: serde_json::Value) -> ParamType {
        ParamType::from_json_schema(&schema, &HashMap::new())
    }

    #[test]
    fn maps_primitives_and_unit() {
        assert!(matches!(pt(json!({"type": "integer"})), ParamType::Integer));
        assert!(matches!(pt(json!({"type": "boolean"})), ParamType::Boolean));
        assert!(matches!(pt(json!({"type": "null"})), ParamType::Unit));
    }

    #[test]
    fn maps_core_refs_in_both_url_forms() {
        for prefix in [
            "https://tx3.land/specs/v1beta0/tii#/$defs",
            "https://tx3.land/specs/v1beta0/core#",
        ] {
            // the legacy form has no trailing slash before the name; the canonical
            // form does — the trailing-name matcher handles both.
            let join = |name: &str| {
                if prefix.ends_with('#') {
                    format!("{prefix}{name}")
                } else {
                    format!("{prefix}/{name}")
                }
            };
            assert!(matches!(pt(json!({"$ref": join("Bytes")})), ParamType::Bytes));
            assert!(matches!(
                pt(json!({"$ref": join("Address")})),
                ParamType::Address
            ));
            assert!(matches!(
                pt(json!({"$ref": join("UtxoRef")})),
                ParamType::UtxoRef
            ));
            assert!(matches!(pt(json!({"$ref": join("Utxo")})), ParamType::Utxo));
            assert!(matches!(
                pt(json!({"$ref": join("AnyAsset")})),
                ParamType::AnyAsset
            ));
        }
    }

    #[test]
    fn maps_list_and_nested_list() {
        match pt(json!({"type": "array", "items": {"type": "integer"}})) {
            ParamType::List(inner) => assert!(matches!(*inner, ParamType::Integer)),
            other => panic!("expected list, got {other:?}"),
        }
        match pt(json!({"type": "array", "items": {"type": "array", "items": {"type": "boolean"}}})) {
            ParamType::List(inner) => match *inner {
                ParamType::List(deep) => assert!(matches!(*deep, ParamType::Boolean)),
                other => panic!("expected list(list), got {other:?}"),
            },
            other => panic!("expected list, got {other:?}"),
        }
    }

    #[test]
    fn maps_tuple_with_prefix_items() {
        let schema = json!({
            "type": "array",
            "prefixItems": [
                {"type": "integer"},
                {"$ref": "https://tx3.land/specs/v1beta0/tii#/$defs/Bytes"}
            ],
            "items": false
        });
        match pt(schema) {
            ParamType::Tuple(els) => {
                assert_eq!(els.len(), 2);
                assert!(matches!(els[0], ParamType::Integer));
                assert!(matches!(els[1], ParamType::Bytes));
            }
            other => panic!("expected tuple, got {other:?}"),
        }
    }

    #[test]
    fn maps_map_via_additional_properties() {
        match pt(json!({"type": "object", "additionalProperties": {"type": "integer"}})) {
            ParamType::Map(value) => assert!(matches!(*value, ParamType::Integer)),
            other => panic!("expected map, got {other:?}"),
        }
    }

    #[test]
    fn maps_record_via_properties() {
        let schema = json!({
            "type": "object",
            "properties": {"price": {"type": "integer"}, "live": {"type": "boolean"}},
            "required": ["price", "live"]
        });
        match pt(schema) {
            ParamType::Record(fields) => {
                assert!(matches!(fields["price"], ParamType::Integer));
                assert!(matches!(fields["live"], ParamType::Boolean));
            }
            other => panic!("expected record, got {other:?}"),
        }
    }

    #[test]
    fn maps_variant_via_one_of() {
        let schema = json!({
            "oneOf": [
                {"type": "object", "additionalProperties": false, "required": ["Buy"],
                 "properties": {"Buy": {"type": "object", "properties": {}, "required": []}}},
                {"type": "object", "additionalProperties": false, "required": ["Sell"],
                 "properties": {"Sell": {"type": "object", "properties": {"price": {"type": "integer"}}, "required": ["price"]}}}
            ]
        });
        match pt(schema) {
            ParamType::Variant(cases) => {
                assert_eq!(cases.len(), 2);
                assert_eq!(cases[0].tag, "Buy");
                assert_eq!(cases[1].tag, "Sell");
                match &*cases[1].fields {
                    ParamType::Record(fields) => {
                        assert!(matches!(fields["price"], ParamType::Integer))
                    }
                    other => panic!("expected record fields, got {other:?}"),
                }
            }
            other => panic!("expected variant, got {other:?}"),
        }
    }

    #[test]
    fn resolves_component_refs_recursively() {
        let mut components = HashMap::new();
        components.insert(
            "AssetClass".to_string(),
            json!({
                "type": "object",
                "properties": {"policy": {"$ref": "https://tx3.land/specs/v1beta0/tii#/$defs/Bytes"}},
                "required": ["policy"]
            }),
        );
        let schema = json!({"$ref": "#/components/schemas/AssetClass"});
        match ParamType::from_json_schema(&schema, &components) {
            ParamType::Record(fields) => assert!(matches!(fields["policy"], ParamType::Bytes)),
            other => panic!("expected record, got {other:?}"),
        }
        // Missing component → Unknown, never panics.
        let missing = json!({"$ref": "#/components/schemas/Nope"});
        assert!(matches!(
            ParamType::from_json_schema(&missing, &components),
            ParamType::Unknown(_)
        ));
    }

    #[test]
    fn unrecognized_shapes_fall_back_to_unknown() {
        assert!(matches!(pt(json!({"type": "string"})), ParamType::Unknown(_)));
        assert!(matches!(pt(json!({})), ParamType::Unknown(_)));
        assert!(matches!(pt(json!("nonsense")), ParamType::Unknown(_)));
        assert!(matches!(
            pt(json!({"$ref": "https://example.com/Weird"})),
            ParamType::Unknown(_)
        ));
        assert!(matches!(
            pt(json!({"type": "array"})),
            ParamType::Unknown(_)
        ));
    }
}
