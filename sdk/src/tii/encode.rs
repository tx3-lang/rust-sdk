//! Type-directed argument encoding into the TRP `TaggedArg` wire form.
//!
//! A resolve request carries an untyped TIR, so the resolver can't recover the
//! structure of an aggregate argument (record, list, tuple, map) on its own. The
//! type lives in the `.tii`, so the SDK walks the resolved [`ParamType`] with the
//! user value and emits the self-describing `TaggedArg` (single-key tagged,
//! recursive — schema in `core/trp/v1beta0/trp.json`, prose in the SDK spec's
//! `api-surface/args.md`); the resolver then decodes it without a schema.
//!
//! [`encode`] runs for every mapped arg as one recursive walk: a top-level
//! scalar comes back bare (the resolver coerces it via the flat type), while
//! aggregates and any nested leaf are tagged.

use serde_json::{json, Value};
use thiserror::Error;

use super::schema::{ParamType, VariantCase};

/// An argument value whose shape does not match its declared [`ParamType`],
/// surfaced before the request is sent rather than as an opaque resolver error.
#[derive(Debug, Error)]
pub enum EncodeError {
    /// A value's JSON kind didn't match what the param type expects.
    #[error("expected {expected} for a `{kind}` argument, got `{got}`")]
    WrongShape {
        /// The `ParamType` kind being encoded (e.g. `list`, `record`).
        kind: &'static str,
        /// The JSON shape that was required (e.g. `array`, `object`).
        expected: &'static str,
        /// The JSON shape actually provided.
        got: String,
    },

    /// A tuple value had the wrong number of elements.
    #[error("tuple arity mismatch: expected {expected} element(s), got {got}")]
    TupleArity {
        /// The declared tuple arity.
        expected: usize,
        /// The arity of the provided value.
        got: usize,
    },

    /// A record value was missing a declared field.
    #[error("missing record field `{0}`")]
    MissingField(String),

    /// A record value carried a field the type does not declare.
    #[error("unknown record field `{0}`")]
    UnknownField(String),

    /// A variant value named a case the type does not declare.
    #[error("unknown variant case `{0}`")]
    UnknownCase(String),

    /// A variant value was not a single-key object naming its case.
    #[error("variant value must be a single-key object naming the case")]
    BadVariant,
}

/// The JSON shape name of a value, for [`EncodeError`] messages.
fn shape_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// Marshals an argument `value` to its TRP wire form, directed by `param`.
///
/// One recursive walk over `(type, value)`. A leaf renders bare at the top level
/// — the resolver coerces it via the param's flat type — and tagged when it sits
/// inside an aggregate, where the resolver has no element type. Aggregates always
/// render to their tagged structural form. Errors if `value`'s shape can't match
/// `param`.
pub fn encode(param: &ParamType, value: &Value) -> Result<Value, EncodeError> {
    marshal(param, value, false)
}

/// `nested` is true when `value` sits inside an aggregate, where scalar leaves
/// must be tagged for the schema-less resolver.
fn marshal(param: &ParamType, value: &Value, nested: bool) -> Result<Value, EncodeError> {
    match param {
        ParamType::Integer => match value {
            Value::Number(_) | Value::String(_) => Ok(leaf("int", value, nested)),
            other => Err(wrong_shape(
                "integer",
                "number or decimal/hex string",
                other,
            )),
        },
        ParamType::Boolean => match value {
            // Same lenient forms the resolver coerces: bool, 0/1, "true"/"false".
            Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(leaf("bool", value, nested)),
            other => Err(wrong_shape("boolean", "bool", other)),
        },
        ParamType::Bytes => match value {
            Value::String(_) | Value::Object(_) => Ok(leaf("bytes", value, nested)),
            // A native byte array (e.g. a codegen `Vec<u8>` param) serializes to
            // a JSON array of integers; canonicalize it to 0x-prefixed hex, the
            // wire form the resolver coerces (SDK spec §3.9).
            Value::Array(items) => {
                let bytes = byte_array(items).ok_or_else(|| {
                    wrong_shape("bytes", "hex string, bytes envelope, or byte array", value)
                })?;
                let hex = Value::String(format!("0x{}", hex::encode(bytes)));
                Ok(leaf("bytes", &hex, nested))
            }
            other => Err(wrong_shape(
                "bytes",
                "hex string, bytes envelope, or byte array",
                other,
            )),
        },
        ParamType::Address => match value {
            Value::String(_) => Ok(leaf("address", value, nested)),
            other => Err(wrong_shape("address", "bech32 or hex string", other)),
        },
        ParamType::UtxoRef => match value {
            Value::String(_) => Ok(leaf("utxoRef", value, nested)),
            other => Err(wrong_shape("utxoRef", "txid#index string", other)),
        },

        // Unit lowers to a nullary struct.
        ParamType::Unit => Ok(json!({ "struct": { "constructor": 0, "fields": [] } })),

        ParamType::List(inner) => {
            let items = value
                .as_array()
                .ok_or_else(|| wrong_shape("list", "array", value))?;
            let encoded = items
                .iter()
                .map(|v| marshal(inner, v, true))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({ "list": encoded }))
        }

        ParamType::Tuple(elem_types) => {
            let items = value
                .as_array()
                .ok_or_else(|| wrong_shape("tuple", "array", value))?;
            if items.len() != elem_types.len() {
                return Err(EncodeError::TupleArity {
                    expected: elem_types.len(),
                    got: items.len(),
                });
            }
            let encoded = elem_types
                .iter()
                .zip(items)
                .map(|(t, v)| marshal(t, v, true))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(json!({ "tuple": encoded }))
        }

        ParamType::Map(value_type) => {
            let obj = value
                .as_object()
                .ok_or_else(|| wrong_shape("map", "object", value))?;
            // The `.tii` erases the key type (JSON object keys are strings), so
            // keys become `string` leaves; sort for a deterministic pair order.
            let mut keys: Vec<&String> = obj.keys().collect();
            keys.sort();
            let pairs = keys
                .into_iter()
                .map(|k| {
                    Ok(json!([
                        json!({ "string": k }),
                        marshal(value_type, &obj[k], true)?
                    ]))
                })
                .collect::<Result<Vec<_>, EncodeError>>()?;
            Ok(json!({ "map": pairs }))
        }

        // Record → constructor 0; variant resolves its case index. Both emit the
        // positional `struct` form.
        ParamType::Record(fields) => Ok(json!({
            "struct": { "constructor": 0, "fields": marshal_record_fields(fields, value)? }
        })),

        ParamType::Variant(cases) => marshal_variant(cases, value),

        // No wire-leaf form and no element types to drive encoding: pass the value
        // through and let the resolver coerce it via the flat type.
        ParamType::Utxo | ParamType::AnyAsset | ParamType::Unknown(_) => Ok(value.clone()),
    }
}

/// Interprets a JSON array as raw bytes: every element must be an integer in
/// `0..=255`. `None` if any element is not.
fn byte_array(items: &[Value]) -> Option<Vec<u8>> {
    items
        .iter()
        .map(|v| v.as_u64().filter(|b| *b <= u8::MAX as u64).map(|b| b as u8))
        .collect()
}

/// Renders a scalar leaf: bare at the top level (the resolver knows the param's
/// type), tagged when nested inside an aggregate (it doesn't).
fn leaf(tag: &str, value: &Value, nested: bool) -> Value {
    if nested {
        json!({ tag: value })
    } else {
        value.clone()
    }
}

fn wrong_shape(kind: &'static str, expected: &'static str, got: &Value) -> EncodeError {
    EncodeError::WrongShape {
        kind,
        expected,
        got: shape_of(got).to_string(),
    }
}

/// Marshals a record's fields **positionally** in declared order, mapping the
/// user's by-name object. Rejects missing or extra fields up front.
fn marshal_record_fields(
    fields: &[(String, ParamType)],
    value: &Value,
) -> Result<Vec<Value>, EncodeError> {
    let obj = value
        .as_object()
        .ok_or_else(|| wrong_shape("record", "object", value))?;

    for key in obj.keys() {
        if !fields.iter().any(|(name, _)| name == key) {
            return Err(EncodeError::UnknownField(key.clone()));
        }
    }

    fields
        .iter()
        .map(|(name, ty)| {
            let field_value = obj
                .get(name)
                .ok_or_else(|| EncodeError::MissingField(name.clone()))?;
            marshal(ty, field_value, true)
        })
        .collect()
}

/// Marshals an externally-tagged variant value `{ "<Case>": <payload> }` into a
/// `struct` whose `constructor` is the case index from the `.tii` `oneOf` order.
fn marshal_variant(cases: &[VariantCase], value: &Value) -> Result<Value, EncodeError> {
    let obj = value.as_object().ok_or(EncodeError::BadVariant)?;
    if obj.len() != 1 {
        return Err(EncodeError::BadVariant);
    }
    let (tag, payload) = obj.iter().next().expect("one entry");

    let index = cases
        .iter()
        .position(|c| &c.tag == tag)
        .ok_or_else(|| EncodeError::UnknownCase(tag.clone()))?;

    let fields = match &*cases[index].fields {
        ParamType::Record(field_types) => marshal_record_fields(field_types, payload)?,
        // Defensive: a non-record payload encodes as a single field.
        other => vec![marshal(other, payload, true)?],
    };

    Ok(json!({ "struct": { "constructor": index, "fields": fields } }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashMap;

    /// Builds a `ParamType` from a JSON schema node + components (mirrors how the
    /// SDK interprets a `.tii`).
    fn param_type(schema: &Value, components: &HashMap<String, Value>) -> ParamType {
        ParamType::from_json_schema(schema, components)
    }

    /// Loads the shared wire-vectors oracle. The vectors live in the umbrella's
    /// `sdk-spec`; this repo also keeps a copy under `tests/fixtures`. We resolve
    /// whichever is reachable so the suite passes both standalone and in-tree.
    fn wire_vectors() -> Value {
        let manifest = env!("CARGO_MANIFEST_DIR");
        let candidates = [
            format!("{manifest}/tests/fixtures/wire-vectors.json"),
            format!("{manifest}/../../sdk-spec/test-vectors/complex-types/wire-vectors.json"),
            format!(
                "{manifest}/../../../sdks/sdk-spec/test-vectors/complex-types/wire-vectors.json"
            ),
        ];
        for path in candidates {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                return serde_json::from_str(&contents).expect("wire-vectors.json parses");
            }
        }
        panic!("could not locate wire-vectors.json in any known path");
    }

    fn components(vectors: &Value) -> HashMap<String, Value> {
        vectors["components"]
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    #[test]
    fn encodes_all_accept_vectors() {
        let vectors = wire_vectors();
        let components = components(&vectors);

        for vector in vectors["accept"].as_array().unwrap() {
            let name = vector["name"].as_str().unwrap();
            let param = param_type(&vector["schema"], &components);
            let got = encode(&param, &vector["value"])
                .unwrap_or_else(|e| panic!("vector `{name}` failed to encode: {e}"));
            assert_eq!(got, vector["tagged"], "vector `{name}` wire mismatch");
        }
    }

    #[test]
    fn rejects_all_reject_vectors() {
        let vectors = wire_vectors();
        let components = components(&vectors);

        for vector in vectors["reject"].as_array().unwrap() {
            let name = vector["name"].as_str().unwrap();
            let param = param_type(&vector["schema"], &components);
            let result = encode(&param, &vector["value"]);
            assert!(
                result.is_err(),
                "vector `{name}` should have been rejected, got {result:?}"
            );
        }
    }

    #[test]
    fn record_field_order_follows_required_not_alphabetical() {
        // Meta { tags: List<Int>, level: Int } — required = [tags, level], while
        // `properties` alphabetizes to [level, tags]. The struct fields must be
        // [list, int], not [int, list].
        let schema = json!({
            "type": "object",
            "properties": {
                "level": { "type": "integer" },
                "tags": { "type": "array", "items": { "type": "integer" } }
            },
            "required": ["tags", "level"]
        });
        let param = param_type(&schema, &HashMap::new());
        let got = encode(&param, &json!({ "level": 7, "tags": [1, 2, 3] })).unwrap();
        assert_eq!(
            got,
            json!({
                "struct": {
                    "constructor": 0,
                    "fields": [{ "list": [{ "int": 1 }, { "int": 2 }, { "int": 3 }] }, { "int": 7 }]
                }
            })
        );
    }

    #[test]
    fn top_level_scalars_render_bare() {
        // A scalar at the top level is sent bare; the resolver coerces it.
        let int = param_type(&json!({ "type": "integer" }), &HashMap::new());
        assert_eq!(encode(&int, &json!(5)).unwrap(), json!(5));

        let bytes = param_type(
            &json!({ "$ref": "https://tx3.land/specs/v1beta0/tii#/$defs/Bytes" }),
            &HashMap::new(),
        );
        assert_eq!(encode(&bytes, &json!("cafe")).unwrap(), json!("cafe"));
    }

    #[test]
    fn nested_scalars_are_tagged() {
        // The same scalar nested inside a list is tagged.
        let list = param_type(
            &json!({ "type": "array", "items": { "type": "integer" } }),
            &HashMap::new(),
        );
        assert_eq!(
            encode(&list, &json!([5])).unwrap(),
            json!({ "list": [{ "int": 5 }] })
        );
    }

    fn bytes_schema() -> Value {
        json!({ "$ref": "https://tx3.land/specs/v1beta0/tii#/$defs/Bytes" })
    }

    fn list_of_bytes_schema() -> Value {
        json!({ "type": "array", "items": bytes_schema() })
    }

    #[test]
    fn native_byte_arrays_canonicalize_to_hex() {
        // A codegen `Vec<u8>` param serializes to a JSON array of integers; the
        // encoder must canonicalize it to 0x-prefixed hex, not send it raw
        // (regression: TRP `(-32005) value is not bytes: [1,1]`).
        let bytes = param_type(&bytes_schema(), &HashMap::new());
        assert_eq!(
            encode(&bytes, &json!([1, 1])).unwrap(),
            json!("0x0101"),
            "top-level byte array renders bare canonical hex"
        );

        let list = param_type(&list_of_bytes_schema(), &HashMap::new());
        assert_eq!(
            encode(&list, &json!([[1, 2]])).unwrap(),
            json!({ "list": [{ "bytes": "0x0102" }] }),
            "nested byte array renders tagged canonical hex"
        );
    }

    #[test]
    fn rejects_non_byte_arrays_for_bytes() {
        let bytes = param_type(&bytes_schema(), &HashMap::new());
        assert!(encode(&bytes, &json!([1, 256])).is_err());
        assert!(encode(&bytes, &json!([1, -1])).is_err());
        assert!(encode(&bytes, &json!(["aa", 1])).is_err());
        assert!(encode(&bytes, &json!(true)).is_err());
    }

    #[test]
    fn hydra_init_arg_shapes() {
        // Hydra `init` from the feedback repro: `participants` / `parties` are
        // `List<Bytes>`, `head_id` is `Bytes`. Both hex-string and native
        // byte-array element forms must produce a CBOR-decodable list of
        // byte-strings, never a bare byte-string and never raw arrays
        // (regression: `(-32005) target type not supported: List` /
        // `value is not bytes: [1,2]`).
        let list = param_type(&list_of_bytes_schema(), &HashMap::new());

        // participants as hex strings
        assert_eq!(
            encode(&list, &json!(["0102", "0304"])).unwrap(),
            json!({ "list": [{ "bytes": "0102" }, { "bytes": "0304" }] })
        );
        // participants as native byte arrays (`vec![vec![1, 2]]`)
        assert_eq!(
            encode(&list, &json!([[1, 2]])).unwrap(),
            json!({ "list": [{ "bytes": "0x0102" }] })
        );
        // parties: same List<Bytes> shape, verifier-key bytes
        assert_eq!(
            encode(&list, &json!([[222, 173, 190, 239]])).unwrap(),
            json!({ "list": [{ "bytes": "0xdeadbeef" }] })
        );
        // head_id: scalar Bytes stays bare at the top level
        let bytes = param_type(&bytes_schema(), &HashMap::new());
        assert_eq!(
            encode(&bytes, &json!("abcd0123")).unwrap(),
            json!("abcd0123")
        );
    }

    #[test]
    fn asteria_name_arg_shapes() {
        // Asteria `create_ship`: `ship_name` / `pilot_name` are `Bytes` params.
        // Hex-string input passes through bare; native byte arrays canonicalize
        // (regression: `(-32005) value is not bytes: [1,1]`).
        let bytes = param_type(&bytes_schema(), &HashMap::new());
        assert_eq!(
            encode(&bytes, &json!("53484950313233")).unwrap(),
            json!("53484950313233")
        );
        assert_eq!(
            encode(&bytes, &json!([83, 72, 73, 80])).unwrap(),
            json!("0x53484950")
        );
    }
}
