use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use thiserror::Error;

use tx3_tir::{interop::json::TirEnvelope, model::v1beta0, reduce::ArgValue};

use crate::tii::spec::Environment;

pub mod spec;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid TII JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("failed to read file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("unknown tx: {0}")]
    UnknownTx(String),

    #[error("unknown environment: {0}")]
    UnknownEnvironment(String),

    #[error(transparent)]
    ReduceError(#[from] tx3_tir::reduce::Error),

    #[error(transparent)]
    InteropError(#[from] tx3_tir::interop::Error),
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

    fn load_tx(&self, key: &str) -> Result<v1beta0::Tx, Error> {
        let tx = self.spec.transactions.get(key);
        let tx = tx.ok_or(Error::UnknownTx(key.to_string()))?;

        let tx = v1beta0::Tx::try_from(tx.tir.clone())?;

        Ok(tx)
    }

    fn ensure_env(&self, key: &str) -> Result<Environment, Error> {
        let env = self
            .spec
            .environments
            .get(key)
            .ok_or_else(|| Error::UnknownEnvironment(key.to_string()))?;

        Ok(env.clone())
    }

    pub fn invoke(&self, tx: &str, env: Option<&str>) -> Result<Invocation, Error> {
        let tx = self.load_tx(tx)?;

        let env = match env {
            Some(x) => self.ensure_env(x)?,
            None => Environment::default(),
        };

        Ok(Invocation::new(tx.clone(), env))
    }

    pub fn txs(&self) -> &HashMap<String, spec::Transaction> {
        &self.spec.transactions
    }
}

pub type ParamMap = BTreeMap<String, v1beta0::Type>;
pub type QueryMap = BTreeMap<String, v1beta0::InputQuery>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Invocation {
    prototype: v1beta0::Tx,
    env: Environment,
    args: BTreeMap<String, ArgValue>,
    inputs: BTreeMap<String, v1beta0::UtxoSet>,
    fees: Option<u64>,

    // Finalized tx
    tx: Option<v1beta0::Tx>,
}

impl Invocation {
    pub fn new(prototype: v1beta0::Tx, env: Environment) -> Self {
        Self {
            prototype,
            env,
            args: BTreeMap::new(),
            inputs: BTreeMap::new(),
            fees: None,
            tx: None,
        }
    }

    fn finalize(&mut self) -> Result<(), Error> {
        let mut tx = self.prototype.clone();
        tx = tx3_tir::reduce::apply_args(tx, &self.args)?;
        tx = tx3_tir::reduce::apply_inputs(tx, &self.inputs)?;

        if let Some(fees) = self.fees {
            tx = tx3_tir::reduce::apply_fees(tx, fees)?;
        }

        tx = tx3_tir::reduce::reduce(tx)?;

        self.tx = Some(tx);

        Ok(())
    }

    fn clear_finalized(&mut self) {
        self.tx = None;
    }

    fn ensure_finalized(&mut self) -> Result<&v1beta0::Tx, Error> {
        if self.tx.is_none() {
            self.finalize()?;
        }

        Ok(self.tx.as_ref().unwrap())
    }

    pub fn define_params(&mut self) -> Result<ParamMap, Error> {
        let tx = self.ensure_finalized()?;
        let params = tx3_tir::reduce::find_params(tx);
        Ok(params)
    }

    pub fn define_queries(&mut self) -> Result<QueryMap, Error> {
        let tx = self.ensure_finalized()?;
        let queries = tx3_tir::reduce::find_queries(tx);
        Ok(queries)
    }

    pub fn set_arg(&mut self, name: &str, value: ArgValue) {
        self.args.insert(name.to_lowercase().to_string(), value);
        self.clear_finalized();
    }

    pub fn set_args(&mut self, args: HashMap<String, ArgValue>) {
        self.args.extend(args);
        self.clear_finalized();
    }

    pub fn with_arg(mut self, name: &str, value: ArgValue) -> Self {
        self.args.insert(name.to_lowercase().to_string(), value);
        self.clear_finalized();
        self
    }

    pub fn with_args(mut self, args: HashMap<String, ArgValue>) -> Self {
        self.args.extend(args);
        self.clear_finalized();
        self
    }

    pub fn set_input(&mut self, name: &str, value: v1beta0::UtxoSet) {
        self.inputs.insert(name.to_lowercase().to_string(), value);
        self.clear_finalized();
    }

    pub fn set_fees(&mut self, value: u64) {
        self.fees = Some(value);
        self.clear_finalized();
    }

    pub fn into_tir(mut self) -> Result<v1beta0::Tx, Error> {
        self.ensure_finalized()?;
        Ok(self.tx.take().unwrap())
    }

    pub fn into_trp_request(self) -> Result<crate::trp::ProtoTxRequest, Error> {
        let args = self
            .args
            .clone()
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();

        let tx = self.into_tir()?;

        let content = tx3_tir::interop::to_vec(&tx);

        let tir = TirEnvelope {
            content: hex::encode(content),
            encoding: tx3_tir::interop::json::BytesEncoding::Hex,
            version: "v1beta0".to_string(),
        };

        Ok(crate::trp::ProtoTxRequest { tir, args })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use tx3_tir::model::assets::CanonicalAssets;

    use super::*;

    #[test]
    fn happy_path() {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let tii = format!("{manifest_dir}/../examples/transfer.tii.json");

        let protocol = Protocol::from_file(&tii).unwrap();

        let invoke = protocol
            .invoke("transfer", Some("cardano-preview"))
            .unwrap();

        let mut invoke = invoke
            .with_arg("sender", ArgValue::Address(b"sender".to_vec()))
            .with_arg("quantity", ArgValue::Int(100_000_000));

        invoke.set_input(
            "source",
            HashSet::from([v1beta0::Utxo {
                r#ref: v1beta0::UtxoRef {
                    txid: b"fafafafafafafafafafafafafafafafafafafafafafafafafafafafafafafafa"
                        .to_vec(),
                    index: 0,
                },
                address: b"abababa".to_vec(),
                datum: None,
                assets: CanonicalAssets::from_defined_asset(b"abababa", b"asset", 100),
                script: Some(v1beta0::Expression::Bytes(b"abce".to_vec())),
            }]),
        );

        dbg!(&invoke.define_params().unwrap());
        dbg!(&invoke.define_queries().unwrap());

        let tx = invoke.into_tir().unwrap();
        dbg!(&tx);
    }
}
