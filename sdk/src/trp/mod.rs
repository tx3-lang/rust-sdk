//! Transaction Resolve Protocol (TRP) Client
//!
//! This module provides a client for interacting with the Transaction Resolve Protocol (TRP),
//! a JSON-RPC based protocol for resolving, submitting, and tracking UTxO transactions.
//!
//! ## Key Features
//!
//! - **Transaction Resolution**: Convert TX3 transaction templates into concrete UTxO transactions
//! - **Transaction Submission**: Submit signed transactions to the network
//! - **Status Monitoring**: Track transaction lifecycle from pending to finalization
//! - **Queue Inspection**: Peek at pending and in-flight transactions
//! - **Log Access**: Query historical transaction logs
//!
//! ## Usage Example
//!
//! ```ignore
//! use tx3_sdk::trp::{Client, ClientOptions, ResolveParams, SubmitParams};
//! use tx3_sdk::core::TirEnvelope;
//!
//! // Create TRP client
//! let client = Client::new(ClientOptions {
//!     endpoint: "https://trp.example.com".to_string(),
//!     headers: None,
//! });
//!
//! // Resolve a transaction
//! let params = ResolveParams {
//!     tir: TirEnvelope { /* ... */ },
//!     args: serde_json::Map::new(),
//!     env: None,
//! };
//!
//! let tx_envelope = client.resolve(params).await?;
//! println!("Resolved transaction hash: {}", tx_envelope.hash);
//!
//! // Check status
//! let status = client.check_status(vec![tx_envelope.hash]).await?;
//! ```

use reqwest::header;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

pub use crate::trp::spec::{
    ChainPoint, CheckStatusResponse, DumpLogsResponse, InflightTx, InputNotResolvedDiagnostic,
    MissingTxArgDiagnostic, PeekInflightResponse, PeekPendingResponse, PendingTx, ResolveParams,
    SubmitParams, SubmitResponse, TxEnvelope, TxLog, TxScriptFailureDiagnostic, TxStage, TxStatus,
    TxStatusMap, TxWitness, UnsupportedTirDiagnostic, WitnessType,
};

mod spec;

/// Error type for TRP client operations.
///
/// This enum represents all possible errors that can occur when interacting
/// with the TRP protocol, including network errors, HTTP errors, deserialization
/// errors, and specific TRP protocol errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Network error from the underlying HTTP client.
    #[error("network error: {0}")]
    NetworkError(#[from] reqwest::Error),

    /// HTTP error with status code and message.
    #[error("HTTP error {0}: {1}")]
    HttpError(u16, String),

    /// Failed to deserialize the response from the server.
    #[error("Failed to deserialize response: {0}")]
    DeserializationError(String),

    /// Generic JSON-RPC error with code, message, and optional data.
    #[error("({0}) {1}")]
    GenericRpcError(i32, String, Option<Value>),

    /// Unknown error with a message.
    #[error("Unknown error: {0}")]
    UnknownError(String),

    /// The TIR version provided is not supported by the server.
    ///
    /// Contains the expected and provided version information.
    #[error("TIR version {provided} is not supported, expected {expected}", provided = .0.provided, expected = .0.expected)]
    UnsupportedTir(UnsupportedTirDiagnostic),

    /// The TIR envelope format is invalid.
    #[error("invalid TIR envelope")]
    InvalidTirEnvelope,

    /// Failed to decode the intermediate representation bytes.
    #[error("failed to decode IR bytes")]
    InvalidTirBytes,

    /// Only transactions from the Conway era are supported.
    #[error("only txs from Conway era are supported")]
    UnsupportedTxEra,

    /// The node cannot resolve transactions while running at the specified era.
    #[error("node can't resolve txs while running at era {era}")]
    UnsupportedEra {
        /// The era that doesn't support transaction resolution.
        era: String,
    },

    /// A required transaction argument is missing.
    ///
    /// Contains the name and expected type of the missing argument.
    #[error("missing argument `{key}` of type {ty}", key = .0.key, ty = .0.arg_type)]
    MissingTxArg(MissingTxArgDiagnostic),

    /// An input could not be resolved during transaction construction.
    ///
    /// Contains diagnostic information about the failed query.
    #[error("input `{name}` not resolved", name = .0.name)]
    InputNotResolved(Box<InputNotResolvedDiagnostic>),

    /// The transaction script execution failed.
    ///
    /// Contains log output from the failed script.
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
                Ok(data) => Error::InputNotResolved(Box::new(data)),
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

/// Configuration options for the TRP client.
///
/// This structure holds the configuration needed to create a TRP client,
/// including the endpoint URL and optional custom headers.
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::trp::ClientOptions;
/// use std::collections::HashMap;
///
/// let mut headers = HashMap::new();
/// headers.insert("Authorization".to_string(), "Bearer token123".to_string());
///
/// let options = ClientOptions {
///     endpoint: "https://trp.example.com".to_string(),
///     headers: Some(headers),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ClientOptions {
    /// The TRP server endpoint URL.
    pub endpoint: String,

    /// Optional custom HTTP headers to include in requests.
    pub headers: Option<HashMap<String, String>>,
}

/// JSON-RPC request structure.
///
/// Internal structure used to serialize JSON-RPC requests to the TRP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// JSON-RPC version (always "2.0").
    pub jsonrpc: String,

    /// The method name to call.
    pub method: String,

    /// The method parameters.
    pub params: serde_json::Value,

    /// Request ID (UUID).
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

/// Client for the Transaction Resolve Protocol (TRP).
///
/// This client provides methods for interacting with a TRP server to resolve
/// transaction templates, submit signed transactions, and monitor transaction
/// status.
///
/// The client is cloneable and can be reused across multiple requests.
///
/// # Example
///
/// ```ignore
/// use tx3_sdk::trp::{Client, ClientOptions};
///
/// let client = Client::new(ClientOptions {
///     endpoint: "https://trp.example.com".to_string(),
///     headers: None,
/// });
///
/// // Use the client for multiple operations
/// let tx = client.resolve(params).await?;
/// let status = client.check_status(vec![tx.hash]).await?;
/// ```
#[derive(Clone)]
pub struct Client {
    options: ClientOptions,
    client: reqwest::Client,
}

impl Client {
    /// Creates a new TRP client with the given options.
    ///
    /// # Arguments
    ///
    /// * `options` - Configuration options including endpoint URL and optional headers
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::{Client, ClientOptions};
    ///
    /// let client = Client::new(ClientOptions {
    ///     endpoint: "https://trp.example.com".to_string(),
    ///     headers: None,
    /// });
    /// ```
    pub fn new(options: ClientOptions) -> Self {
        Self {
            options,
            client: reqwest::Client::new(),
        }
    }

    /// Makes a raw JSON-RPC call to the TRP server.
    ///
    /// This is a low-level method for making JSON-RPC calls. Generally, you should
    /// use the higher-level methods like `resolve`, `submit`, etc.
    ///
    /// # Arguments
    ///
    /// * `method` - The JSON-RPC method name
    /// * `params` - The method parameters as a JSON value
    ///
    /// # Returns
    ///
    /// Returns the result as a JSON value on success, or an error on failure.
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

    /// Resolves a transaction template into a concrete transaction.
    ///
    /// This method takes a Transaction Intermediate Representation (TIR) envelope
    /// and arguments, and resolves it into a concrete UTxO transaction ready
    /// for signing.
    ///
    /// # Arguments
    ///
    /// * `request` - The resolve parameters including TIR and arguments
    ///
    /// # Returns
    ///
    /// Returns a `TxEnvelope` containing the resolved transaction hash and CBOR bytes.
    ///
    /// # Errors
    ///
    /// Can return various errors including:
    /// - `Error::UnsupportedTir` if the TIR version is not supported
    /// - `Error::MissingTxArg` if required arguments are missing
    /// - `Error::InputNotResolved` if an input cannot be found
    /// - `Error::TxScriptFailure` if script execution fails
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::{Client, ResolveParams};
    /// use tx3_sdk::core::TirEnvelope;
    ///
    /// let client = Client::new(/* ... */);
    ///
    /// let params = ResolveParams {
    ///     tir: TirEnvelope { /* ... */ },
    ///     args: serde_json::Map::new(),
    ///     env: None,
    /// };
    ///
    /// let tx = client.resolve(params).await?;
    /// println!("Resolved hash: {}", tx.hash);
    /// ```
    pub async fn resolve(&self, request: ResolveParams) -> Result<TxEnvelope, Error> {
        let params = serde_json::to_value(request).unwrap();

        let response = self.call("trp.resolve", params).await?;

        // Return result
        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    /// Submits a signed transaction to the network.
    ///
    /// This method submits a signed transaction with its witnesses to the
    /// blockchain network via the TRP server.
    ///
    /// # Arguments
    ///
    /// * `request` - The submit parameters including transaction bytes and witnesses
    ///
    /// # Returns
    ///
    /// Returns a `SubmitResponse` containing the submitted transaction hash.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::{Client, SubmitParams, TxWitness, WitnessType};
    /// use tx3_sdk::core::BytesEnvelope;
    ///
    /// let client = Client::new(/* ... */);
    ///
    /// let params = SubmitParams {
    ///     tx: BytesEnvelope { /* signed tx */ },
    ///     witnesses: vec![TxWitness { /* ... */ }],
    /// };
    ///
    /// let response = client.submit(params).await?;
    /// println!("Submitted: {}", response.hash);
    /// ```
    pub async fn submit(&self, request: SubmitParams) -> Result<SubmitResponse, Error> {
        let params = serde_json::to_value(request).unwrap();

        let response = self.call("trp.submit", params).await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    /// Checks the status of one or more transactions.
    ///
    /// This method queries the TRP server for the current status of the
    /// specified transactions.
    ///
    /// # Arguments
    ///
    /// * `hashes` - Vector of transaction hashes to check
    ///
    /// # Returns
    ///
    /// Returns a `CheckStatusResponse` containing a map of transaction hashes
    /// to their current status.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::Client;
    ///
    /// let client = Client::new(/* ... */);
    ///
    /// let hashes = vec!["abc123...".to_string()];
    /// let status = client.check_status(hashes).await?;
    ///
    /// for (hash, tx_status) in status.statuses {
    ///     println!("{}: {:?}", hash, tx_status.stage);
    /// }
    /// ```
    pub async fn check_status(&self, hashes: Vec<String>) -> Result<CheckStatusResponse, Error> {
        let params = serde_json::json!({ "hashes": hashes });

        let response = self.call("trp.checkStatus", params).await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    /// Dumps transaction logs with optional pagination.
    ///
    /// This method retrieves a paginated list of transaction log entries,
    /// useful for monitoring and auditing transaction history.
    ///
    /// # Arguments
    ///
    /// * `cursor` - Optional pagination cursor for fetching specific pages
    /// * `limit` - Optional limit on the number of entries to return
    /// * `include_payload` - Whether to include transaction payloads in the response
    ///
    /// # Returns
    ///
    /// Returns a `DumpLogsResponse` containing log entries and an optional
    /// next cursor for pagination.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::Client;
    ///
    /// let client = Client::new(/* ... */);
    ///
    /// // Get first page with 100 entries
    /// let logs = client.dump_logs(None, Some(100), Some(false)).await?;
    ///
    /// for entry in logs.entries {
    ///     println!("{}: {:?}", entry.hash, entry.stage);
    /// }
    ///
    /// // Get next page if available
    /// if let Some(next) = logs.next_cursor {
    ///     let more_logs = client.dump_logs(Some(next), Some(100), Some(false)).await?;
    /// }
    /// ```
    pub async fn dump_logs(
        &self,
        cursor: Option<u64>,
        limit: Option<u64>,
        include_payload: Option<bool>,
    ) -> Result<DumpLogsResponse, Error> {
        let mut params = serde_json::Map::new();
        if let Some(cursor) = cursor {
            params.insert("cursor".to_string(), serde_json::json!(cursor));
        }
        if let Some(limit) = limit {
            params.insert("limit".to_string(), serde_json::json!(limit));
        }
        if let Some(include_payload) = include_payload {
            params.insert(
                "includePayload".to_string(),
                serde_json::json!(include_payload),
            );
        }

        let response = self
            .call("trp.dumpLogs", serde_json::Value::Object(params))
            .await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    /// Peeks at pending transactions in the mempool.
    ///
    /// This method retrieves pending transactions that are waiting to be
    /// included in a block, useful for monitoring mempool state.
    ///
    /// # Arguments
    ///
    /// * `limit` - Optional limit on the number of pending transactions to return
    /// * `include_payload` - Whether to include transaction payloads in the response
    ///
    /// # Returns
    ///
    /// Returns a `PeekPendingResponse` containing pending transactions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::Client;
    ///
    /// let client = Client::new(/* ... */);
    ///
    /// let pending = client.peek_pending(Some(50), Some(false)).await?;
    ///
    /// println!("Found {} pending transactions", pending.entries.len());
    /// if pending.has_more {
    ///     println!("More transactions available");
    /// }
    /// ```
    pub async fn peek_pending(
        &self,
        limit: Option<u64>,
        include_payload: Option<bool>,
    ) -> Result<PeekPendingResponse, Error> {
        let mut params = serde_json::Map::new();
        if let Some(limit) = limit {
            params.insert("limit".to_string(), serde_json::json!(limit));
        }
        if let Some(include_payload) = include_payload {
            params.insert(
                "includePayload".to_string(),
                serde_json::json!(include_payload),
            );
        }

        let response = self
            .call("trp.peekPending", serde_json::Value::Object(params))
            .await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }

    /// Peeks at in-flight transactions being tracked by the server.
    ///
    /// This method retrieves transactions that have been submitted and are
    /// being tracked through their lifecycle stages.
    ///
    /// # Arguments
    ///
    /// * `limit` - Optional limit on the number of in-flight transactions to return
    /// * `include_payload` - Whether to include transaction payloads in the response
    ///
    /// # Returns
    ///
    /// Returns a `PeekInflightResponse` containing in-flight transactions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use tx3_sdk::trp::Client;
    ///
    /// let client = Client::new(/* ... */);
    ///
    /// let inflight = client.peek_inflight(Some(50), Some(false)).await?;
    ///
    /// for tx in inflight.entries {
    ///     println!("{}: {:?} ({} confirmations)",
    ///         tx.hash, tx.stage, tx.confirmations);
    /// }
    /// ```
    pub async fn peek_inflight(
        &self,
        limit: Option<u64>,
        include_payload: Option<bool>,
    ) -> Result<PeekInflightResponse, Error> {
        let mut params = serde_json::Map::new();
        if let Some(limit) = limit {
            params.insert("limit".to_string(), serde_json::json!(limit));
        }
        if let Some(include_payload) = include_payload {
            params.insert(
                "includePayload".to_string(),
                serde_json::json!(include_payload),
            );
        }

        let response = self
            .call("trp.peekInflight", serde_json::Value::Object(params))
            .await?;

        let out = serde_json::from_value(response)
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        Ok(out)
    }
}
