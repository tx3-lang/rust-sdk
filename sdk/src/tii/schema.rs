//! Interpretation of the TII params JSON schema into [`ParamType`] kinds.
//!
//! A TII embeds, for each transaction, a JSON schema describing its parameters
//! (and an optional environment schema). This module turns those schema nodes —
//! every shape `tx3c` can emit, see the SDK spec's `api-surface/args.md` — into
//! the [`ParamType`] model the rest of the SDK works with. Interpretation never
//! fails: any shape it does not recognize becomes [`ParamType::Unknown`].

use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// Map of parameter names to their types.
///
/// Used to represent the complete set of parameters required for a transaction.
pub type ParamMap = HashMap<String, ParamType>;

/// Builds a parameter-type map from a JSON schema's `properties`. Never fails:
/// unrecognized property schemas yield [`ParamType::Unknown`]. `components` is the
/// TII's `components.schemas` table, used to resolve `#/components/schemas/<Name>`
/// refs to user-defined record / variant types.
pub(super) fn params_from_schema(schema: &Value, components: &HashMap<String, Value>) -> ParamMap {
    let mut params = ParamMap::new();

    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        for (key, value) in properties {
            params.insert(key.clone(), ParamType::from_json_schema(value, components));
        }
    }

    params
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

    /// Resolves a `$ref` node: `#/components/schemas/<Name>` against the TII's
    /// `components` table (recursing into the resolved schema), otherwise a
    /// built-in core ref. An unresolved ref becomes [`ParamType::Unknown`].
    fn ref_type(schema: &Value, reference: &str, components: &HashMap<String, Value>) -> ParamType {
        if let Some(name) = reference.strip_prefix("#/components/schemas/") {
            return match components.get(name) {
                Some(resolved) => Self::from_json_schema(resolved, components),
                None => ParamType::Unknown(schema.clone()),
            };
        }

        Self::core_ref_type(reference).unwrap_or_else(|| ParamType::Unknown(schema.clone()))
    }

    /// Maps a `oneOf` array to a [`ParamType::Variant`] of externally-tagged cases.
    fn variant_type(cases: &[Value], components: &HashMap<String, Value>) -> ParamType {
        ParamType::Variant(
            cases
                .iter()
                .map(|case| Self::variant_case(case, components))
                .collect(),
        )
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

    /// Maps an `array` schema: `prefixItems` → [`ParamType::Tuple`], `items` →
    /// [`ParamType::List`]. An array carrying neither becomes [`ParamType::Unknown`].
    fn array_type(schema: &Value, components: &HashMap<String, Value>) -> ParamType {
        if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
            ParamType::Tuple(
                prefix
                    .iter()
                    .map(|el| Self::from_json_schema(el, components))
                    .collect(),
            )
        } else if let Some(items) = schema.get("items").filter(|i| i.is_object()) {
            ParamType::List(Box::new(Self::from_json_schema(items, components)))
        } else {
            ParamType::Unknown(schema.clone())
        }
    }

    /// Maps an `object` schema: `additionalProperties` → [`ParamType::Map`],
    /// `properties` → [`ParamType::Record`]. Neither present → [`ParamType::Unknown`].
    fn object_type(schema: &Value, components: &HashMap<String, Value>) -> ParamType {
        if let Some(value) = schema.get("additionalProperties").filter(|v| v.is_object()) {
            ParamType::Map(Box::new(Self::from_json_schema(value, components)))
        } else if let Some(props) = schema.get("properties").and_then(Value::as_object) {
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
            return Self::ref_type(schema, reference, components);
        }

        if let Some(cases) = obj.get("oneOf").and_then(Value::as_array) {
            return Self::variant_type(cases, components);
        }

        match obj.get("type").and_then(Value::as_str) {
            Some("integer") => ParamType::Integer,
            Some("boolean") => ParamType::Boolean,
            Some("null") => ParamType::Unit,
            Some("array") => Self::array_type(schema, components),
            Some("object") => Self::object_type(schema, components),
            _ => ParamType::Unknown(schema.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
