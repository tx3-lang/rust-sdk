use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub mod args;
pub mod error;

pub use args::ArgValue;

use crate::trp::args::BytesEnvelope;

// Custom error type for TRP operations
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    #[error("HTTP error {0}: {1}")]
    StatusCodeError(u16, String),

    #[error("Failed to deserialize response: {0}")]
    DeserializationError(String),

    #[error("JSON-RPC error: {1}")]
    JsonRpcError(String, String),

    #[error("Unknown error: {0}")]
    UnknownError(String),
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
    message: String,
    data: Option<Value>,
}

/// Client for the Transaction Resolve Protocol (TRP)
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

        // Check if response is successful
        if !response.status().is_success() {
            return Err(Error::StatusCodeError(
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
            return Err(Error::JsonRpcError(
                error.message,
                error
                    .data
                    .map_or_else(|| "No data".to_string(), |v| v.to_string()),
            ));
        }

        // Return result
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

        // Return result
        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }
}
