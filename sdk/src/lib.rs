/*!
 * TX3 SDK for Rust
 * 
 * This SDK provides tools for interacting with TX3 services.
 * Currently includes support for the Transaction Resolve Protocol (TRP).
 * 
 * ## Usage Example
 * 
 * ```
 * use tx3_sdk::trp::{Client, ClientOptions};
 * 
 * // Create TRP client
 * let client = Client::new(ClientOptions {
 *     endpoint: "https://trp.example.com".to_string(),
 *     headers: None,
 *     env_args: None,
 * });
 * ```
 */

pub mod trp;
pub use tx3_lang;