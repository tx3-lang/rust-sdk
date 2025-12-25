//! Transaction Resolve Protocol (TRP) Client
//!
//! This SDK provides tools for interacting with TX3 services.
//! Currently includes support for the Transaction Resolve Protocol (TRP).
//!
//! ## Usage Example
//!
//! ```
//! use tx3_sdk::trp::{Client, ClientOptions};
//!
//! // Create TRP client
//! let client = Client::new(ClientOptions {
//!     endpoint: "https://trp.example.com".to_string(),
//!     headers: None,
//! });
//! ```
//!

use reqwest::header;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

pub use crate::trp::spec::{
    InputNotResolvedDiagnostic, MissingTxArgDiagnostic, ResolveParams, SubmitParams,
    SubmitResponse, SubmitWitness, TxEnvelope, TxScriptFailureDiagnostic, UnsupportedTirDiagnostic,
};

mod spec;

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

    #[error("TIR version {provided} is not supported, expected {expected}", provided = .0.provided, expected = .0.expected)]
    UnsupportedTir(UnsupportedTirDiagnostic),

    #[error("invalid TIR envelope")]
    InvalidTirEnvelope,

    #[error("failed to decode IR bytes")]
    InvalidTirBytes,

    #[error("only txs from Conway era are supported")]
    UnsupportedTxEra,

    #[error("node can't resolve txs while running at era {era}")]
    UnsupportedEra { era: String },

    #[error("missing argument `{key}` of type {ty}", key = .0.key, ty = .0.ty)]
    MissingTxArg(MissingTxArgDiagnostic),

    #[error("input `{name}` not resolved", name = .0.name)]
    InputNotResolved(InputNotResolvedDiagnostic),

    #[error("tx script returned failure")]
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

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
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

    pub async fn resolve(&self, request: ResolveParams) -> Result<TxEnvelope, Error> {
        let params = serde_json::to_value(request).unwrap();

        let response = self.call("trp.resolve", params).await?;

        // Return result
        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    pub async fn submit(&self, request: SubmitParams) -> Result<SubmitResponse, Error> {
        let params = serde_json::to_value(request).unwrap();

        let response = self.call("trp.submit", params).await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }
}
