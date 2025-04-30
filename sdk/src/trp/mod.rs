use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value, Map};
use tx3_lang::ArgValue;
use std::collections::HashMap;
use uuid::Uuid;

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
pub struct TxEnvelope {
    pub tx: String
}

#[derive(Debug, Clone)]
pub struct ClientOptions {
    pub endpoint: String,
    pub headers: Option<HashMap<String, String>>,
    pub env_args: Option<HashMap<String, ArgValue>>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    result: Option<TxEnvelope>,
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
    pub args: serde_json::Value,
}

impl Client {
    pub fn new(options: ClientOptions) -> Self {
        Self {
            options,
            client: reqwest::Client::new(),
        }
    }

    pub async fn resolve(&self, proto_tx: ProtoTxRequest) -> Result<TxEnvelope, Error> {
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
        let body = json!({
            "jsonrpc": "2.0",
            "method": "trp.resolve",
            "params": {
                "tir": proto_tx.tir,
                "args": flatten_json_values(proto_tx.args),
                "env": self.options.env_args,
            },
            "id": Uuid::new_v4().to_string(),
        });

        // Send request
        let response = self.client
            .post(&self.options.endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(Error::from)?;

        // Check if response is successful
        if !response.status().is_success() {
            return Err(Error::StatusCodeError(
                response.status().as_u16(),
                response.status().to_string()
            ));
        }

        // Parse response
        let result: JsonRpcResponse = response.json().await
            .map_err(|e| Error::DeserializationError(e.to_string()))?;

        // Handle possible error
        if let Some(error) = result.error {
            return Err(Error::JsonRpcError(
                error.message,
                error.data
                    .map_or_else(|| "No data".to_string(), |v| v.to_string()),
            ));
        }

        // Return result
        result.result.ok_or_else(|| Error::UnknownError("No result in response".to_string()))
    }
}

/// Recursively flattens nested JSON objects containing ArgValue patterns
fn flatten_json_values(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            // Check if this is an ArgValue pattern (e.g., {"String": "value"})
            if map.len() == 1 && map.contains_key("String") {
                // Extract the inner value
                if let Some(Value::String(s)) = map.get("String") {
                    return Value::String(s.clone());
                }
            }
            
            // Process each field in the object
            let mut new_map = Map::new();
            for (key, val) in map {
                new_map.insert(key, flatten_json_values(val));
            }
            Value::Object(new_map)
        },
        Value::Array(arr) => {
            // Process each element in the array
            Value::Array(
                arr.into_iter()
                    .map(flatten_json_values)
                    .collect()
            )
        },
        // Return other value types as is
        _ => value
    }
}