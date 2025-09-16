use reqwest::header;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

pub mod args;

pub use args::ArgValue;

use crate::trp::args::BytesEnvelope;

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchSpaceDiagnostic {
    pub matched: Vec<String>,
    pub by_address_count: Option<usize>,
    pub by_asset_class_count: Option<usize>,
    pub by_ref_count: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputQueryDiagnostic {
    pub address: Option<String>,
    pub min_amount: HashMap<String, String>,
    pub refs: Vec<String>,
    pub support_many: bool,
    pub collateral: bool,
}

#[derive(Debug, Serialize, Deserialize, Error)]
#[error("input `{name}` not resolved")]
pub struct InputNotResolvedDiagnostic {
    pub name: String,
    pub query: InputQueryDiagnostic,
    pub search_space: SearchSpaceDiagnostic,
}

#[derive(Debug, Serialize, Deserialize, Error)]
#[error("TIR version {provided} is not supported, expected {expected}")]
pub struct UnsupportedTirDiagnostic {
    pub provided: String,
    pub expected: String,
}

#[derive(Debug, Serialize, Deserialize, Error)]
#[error("tx script returned failure")]
pub struct TxScriptFailureDiagnostic {
    pub logs: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Error)]
#[error("missing argument `{key}` of type {ty}")]
pub struct MissingTxArgDiagnostic {
    pub key: String,
    #[serde(rename = "type")]
    pub ty: String,
}

// Custom error type for TRP operations
#[derive(Debug, Error)]
pub enum Error {
    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("HTTP error {0}: {1}")]
    HttpError(u16, String),

    #[error("Failed to deserialize response: {0}")]
    DeserializationError(String),

    #[error("({0}) {1}")]
    GenericRpcError(i32, String, Option<Value>),

    #[error("Unknown error: {0}")]
    UnknownError(String),

    #[error(transparent)]
    UnsupportedTir(UnsupportedTirDiagnostic),

    #[error("invalid TIR envelope")]
    InvalidTirEnvelope,

    #[error("failed to decode IR bytes")]
    InvalidTirBytes,

    #[error("only txs from Conway era are supported")]
    UnsupportedTxEra,

    #[error("node can't resolve txs while running at era {era}")]
    UnsupportedEra { era: String },

    #[error(transparent)]
    MissingTxArg(MissingTxArgDiagnostic),

    #[error(transparent)]
    InputNotResolved(InputNotResolvedDiagnostic),

    #[error(transparent)]
    TxScriptFailure(TxScriptFailureDiagnostic),
}

impl Error {
    fn generic(payload: JsonRpcError) -> Self {
        Self::GenericRpcError(payload.code, payload.message, payload.data)
    }
}

fn expect_json_rpc_error_data<T: DeserializeOwned>(payload: JsonRpcError) -> Result<T, Error> {
    let Some(data) = payload.data.clone() else {
        return Err(Error::generic(payload));
    };

    let Ok(data) = serde_json::from_value(data.clone()) else {
        return Err(Error::generic(payload));
    };

    Ok(data)
}

impl From<JsonRpcError> for Error {
    fn from(error: JsonRpcError) -> Self {
        match error.code {
            -32000 => match expect_json_rpc_error_data(error) {
                Ok(data) => Error::UnsupportedTir(data),
                Err(e) => e,
            },
            -32001 => match expect_json_rpc_error_data(error) {
                Ok(data) => Error::MissingTxArg(data),
                Err(e) => e,
            },
            -32002 => match expect_json_rpc_error_data(error) {
                Ok(data) => Error::InputNotResolved(data),
                Err(e) => e,
            },
            -32003 => match expect_json_rpc_error_data(error) {
                Ok(data) => Error::TxScriptFailure(data),
                Err(e) => e,
            },
            _ => Error::generic(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TirInfo {
    pub version: String,
    pub bytecode: String,
    pub encoding: String, // "base64" | "hex" | other
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VKeyWitness {
    pub key: args::BytesEnvelope,
    pub signature: args::BytesEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SubmitWitness {
    #[serde(rename = "vkey")]
    VKey(VKeyWitness),
}

#[derive(Deserialize, Debug, Serialize)]
pub struct SubmitParams {
    pub tx: args::BytesEnvelope,
    pub witnesses: Vec<SubmitWitness>,
}

#[derive(Deserialize, Debug, Serialize)]
pub struct SubmitResponse {
    pub hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxEnvelope {
    pub tx: String,
    pub hash: String,
}

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
    pub env_args: Option<HashMap<String, ArgValue>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    data: Option<Value>,
}

/// Client for the Transaction Resolve Protocol (TRP)
#[derive(Clone)]
pub struct Client {
    options: ClientOptions,
    client: reqwest::Client,
}

pub struct ProtoTxRequest {
    pub tir: TirInfo,
    pub args: HashMap<String, ArgValue>,
}

impl Client {
    pub fn new(options: ClientOptions) -> Self {
        Self {
            options,
            client: reqwest::Client::new(),
        }
    }

    pub async fn call(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, Error> {
        // Prepare headers
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );

        if let Some(user_headers) = &self.options.headers {
            for (key, value) in user_headers {
                if let Ok(header_name) = header::HeaderName::from_bytes(key.as_bytes()) {
                    if let Ok(header_value) = header::HeaderValue::from_str(value) {
                        headers.insert(header_name, header_value);
                    }
                }
            }
        }

        // Prepare request body with FlattenedArgs for proper serialization
        let body = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            params,
            id: Uuid::new_v4().to_string(),
        };

        // Send request
        let response = self
            .client
            .post(&self.options.endpoint)
            .headers(headers)
            .json(&serde_json::to_value(body).unwrap())
            .send()
            .await
            .map_err(Error::from)?;

        // If the response at the HTTP level is not successful, return an error
        if !response.status().is_success() {
            return Err(Error::HttpError(
                response.status().as_u16(),
                response.status().to_string(),
            ));
        }

        // Parse response
        let result: JsonRpcResponse = response
            .json()
            .await
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        // Handle possible error
        if let Some(error) = result.error {
            return Err(Error::from(error));
        }

        result
            .result
            .ok_or_else(|| Error::UnknownError("No result in response".to_string()))
    }

    pub async fn resolve(&self, proto_tx: ProtoTxRequest) -> Result<TxEnvelope, Error> {
        let params = json!({
            "tir": proto_tx.tir,
            "args": HashMap::<String, serde_json::Value>::from_iter(proto_tx.args.into_iter().map(|(k, v)| (k, args::to_json(v)))),
            "env": self.options.env_args,
        });

        let response = self.call("trp.resolve", params).await?;

        // Return result
        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    pub async fn submit(
        &self,
        tx: TxEnvelope,
        witnesses: Vec<SubmitWitness>,
    ) -> Result<SubmitResponse, Error> {
        let params = serde_json::to_value(SubmitParams {
            tx: BytesEnvelope::from_hex(&tx.tx).unwrap(),
            witnesses,
        })
        .unwrap();

        let response = self.call("trp.submit", params).await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }
}
