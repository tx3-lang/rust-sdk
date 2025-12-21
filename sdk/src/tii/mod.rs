use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

use tx3_tir::{
    model::v1beta0::{self as tir, UtxoSet},
    reduce::ArgValue,
};

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

    fn load_tx(&self, key: &str) -> Result<tir::Tx, Error> {
        let tx = self.spec.transactions.get(key);
        let tx = tx.ok_or(Error::UnknownTx(key.to_string()))?;

        let tx = tir::Tx::try_from(tx.tir.clone())?;

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

    pub fn invoke(&self, tx: &str, env: &str) -> Result<Invocation, Error> {
        let tx = self.load_tx(tx)?;
        let env = self.ensure_env(env)?;
        Ok(Invocation::new(tx.clone(), env.clone()))
    }
}

pub type ParamMap = BTreeMap<String, tir::Type>;
pub type QueryMap = BTreeMap<String, tir::InputQuery>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Invocation {
    prototype: tir::Tx,
    env: Environment,
    args: BTreeMap<String, ArgValue>,
    inputs: BTreeMap<String, UtxoSet>,
    fees: Option<u64>,

    // Finalized tx
    tx: Option<tir::Tx>,
}

impl Invocation {
    pub fn new(prototype: tir::Tx, env: Environment) -> Self {
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

    fn ensure_finalized(&mut self) -> Result<&tir::Tx, Error> {
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

    pub fn with_arg(mut self, name: &str, value: ArgValue) -> Self {
        self.args.insert(name.to_lowercase().to_string(), value);
        self.clear_finalized();
        self
    }

    pub fn set_input(&mut self, name: &str, value: UtxoSet) {
        self.inputs.insert(name.to_lowercase().to_string(), value);
        self.clear_finalized();
    }

    pub fn set_fees(&mut self, value: u64) {
        self.fees = Some(value);
        self.clear_finalized();
    }

    pub fn into_tir(mut self) -> Result<tir::Tx, Error> {
        self.ensure_finalized()?;
        Ok(self.tx.take().unwrap())
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

        let invoke = protocol.invoke("transfer", "cardano-preview").unwrap();

        let mut invoke = invoke
            .with_arg("sender", ArgValue::Address(b"sender".to_vec()))
            .with_arg("quantity", ArgValue::Int(100_000_000));

        invoke.set_input(
            "source",
            HashSet::from([tir::Utxo {
                r#ref: tir::UtxoRef {
                    txid: b"fafafafafafafafafafafafafafafafafafafafafafafafafafafafafafafafa"
                        .to_vec(),
                    index: 0,
                },
                address: b"abababa".to_vec(),
                datum: None,
                assets: CanonicalAssets::from_defined_asset(b"abababa", b"asset", 100),
                script: Some(tir::Expression::Bytes(b"abce".to_vec())),
            }]),
        );

        dbg!(&invoke.define_params().unwrap());
        dbg!(&invoke.define_queries().unwrap());

        let tx = invoke.into_tir().unwrap();
        dbg!(&tx);
    }
}
