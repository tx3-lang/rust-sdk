use schemars::schema::{InstanceType, Schema, SingleOrVec};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use thiserror::Error;

use crate::{
    core::{ArgMap, TirEnvelope},
    tii::spec::{Profile, Transaction},
};

pub mod spec;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid TII JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("failed to read file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("unknown tx: {0}")]
    UnknownTx(String),

    #[error("unknown profile: {0}")]
    UnknownProfile(String),

    #[error("invalid params schema")]
    InvalidParamsSchema,

    #[error("invalid param type")]
    InvalidParamType,
}

fn params_from_schema(schema: Schema) -> Result<ParamMap, Error> {
    let mut params = ParamMap::new();

    let as_object = schema.into_object();

    for (key, value) in as_object.object.unwrap().properties {
        params.insert(key, ParamType::from_json_schema(value)?);
    }

    Ok(params)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Protocol {
    spec: spec::TiiFile,
}

impl Protocol {
    pub fn from_json(json: serde_json::Value) -> Result<Protocol, Error> {
        let spec = serde_json::from_value(json)?;

        Ok(Protocol { spec })
    }

    pub fn from_string(code: String) -> Result<Protocol, Error> {
        let json = serde_json::from_str(&code)?;
        Self::from_json(json)
    }

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
        }

        Ok(out)
    }

    pub fn txs(&self) -> &HashMap<String, spec::Transaction> {
        &self.spec.transactions
    }
}

#[derive(Debug, Clone)]
pub enum ParamType {
    Bytes,
    Integer,
    Boolean,
    UtxoRef,
    Address,
    List(Box<ParamType>),
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

pub struct InputQuery {}

pub type ParamMap = HashMap<String, ParamType>;
pub type QueryMap = BTreeMap<String, InputQuery>;

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
    pub fn params(&mut self) -> &ParamMap {
        &self.params
    }

    pub fn unspecified_params(&mut self) -> impl Iterator<Item = (&String, &ParamType)> {
        self.params
            .iter()
            .filter(|(k, _)| !self.args.contains_key(k.as_str()))
    }

    pub fn set_arg(&mut self, name: &str, value: serde_json::Value) {
        self.args.insert(name.to_lowercase().to_string(), value);
    }

    pub fn set_args(&mut self, args: ArgMap) {
        self.args.extend(args);
    }

    pub fn with_arg(mut self, name: &str, value: serde_json::Value) -> Self {
        self.args.insert(name.to_lowercase().to_string(), value);
        self
    }

    pub fn with_args(mut self, args: ArgMap) -> Self {
        self.args.extend(args);
        self
    }

    pub fn into_resolve_request(self) -> Result<crate::trp::ResolveParams, Error> {
        let args = self
            .args
            .clone()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();

        let tir = self.tir.clone();

        Ok(crate::trp::ResolveParams { tir, args })
    }
}

#[cfg(test)]
mod tests {

    use serde_json::json;

    use super::*;

    #[test]
    fn happy_path_smoke_test() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let tii = format!("{manifest_dir}/../examples/transfer.tii.json");

        let protocol = Protocol::from_file(&tii).unwrap();

        let invoke = protocol.invoke("transfer", Some("preview")).unwrap();

        let mut invoke = invoke
            .with_arg("sender", json!("addr1abc"))
            .with_arg("quantity", json!(100_000_000));

        let all_params: HashSet<_> = invoke.params().keys().collect();

        assert_eq!(all_params.len(), 5);
        assert!(all_params.contains(&"sender".to_string()));
        assert!(all_params.contains(&"quantity".to_string()));
        assert!(all_params.contains(&"middleman".to_string()));
        assert!(all_params.contains(&"receiver".to_string()));
        assert!(all_params.contains(&"tax".to_string()));

        let unspecified_params: HashSet<_> = invoke.unspecified_params().map(|(k, _)| k).collect();

        assert_eq!(unspecified_params.len(), 2);
        assert!(unspecified_params.contains(&"receiver".to_string()));
        assert!(unspecified_params.contains(&"middleman".to_string()));

        let tx = invoke.into_resolve_request().unwrap();

        dbg!(&tx);
    }
}
