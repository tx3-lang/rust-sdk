# tx3-sdk (Rust)

[![crates.io](https://img.shields.io/crates/v/tx3-sdk.svg)](https://crates.io/crates/tx3-sdk)
[![CI](https://github.com/tx3-lang/rust-sdk/actions/workflows/ci.yml/badge.svg)](https://github.com/tx3-lang/rust-sdk/actions/workflows/ci.yml)
[![docs.rs](https://img.shields.io/docsrs/tx3-sdk)](https://docs.rs/tx3-sdk)
[![Tx3 docs](https://img.shields.io/badge/Tx3-docs-blue.svg)](https://docs.txpipe.io/tx3)

The official Rust SDK for [Tx3](https://docs.txpipe.io/tx3) — a DSL and protocol suite for defining and executing UTxO-based blockchain transactions declaratively. Load a compiled `.tii` protocol, bind parties and signers, and drive the full transaction lifecycle (resolve, sign, submit, confirm) via the Transaction Resolve Protocol (TRP).

This repository is organized as a monorepo. The publishable crate lives in `sdk/`.

## What is Tx3

Tx3 is a domain-specific language and protocol suite for declarative, type-safe UTxO transactions. Authors write `.tx3` files describing parties, environment, and transactions; the toolchain compiles them to `.tii` artifacts that this SDK loads at runtime to drive the resolve → sign → submit → wait lifecycle through a TRP server. See the [Tx3 docs](https://docs.txpipe.io/tx3) for project context.

## Installation

```bash
cargo add tx3-sdk
```

Or in `Cargo.toml`:

```toml
[dependencies]
tx3-sdk = "0.12"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

## Quick start

```rust
use serde_json::json;
use tx3_sdk::{CardanoSigner, Party, PollConfig};

#[tokio::main]
async fn main() -> Result<(), tx3_sdk::Error> {
    let signer = CardanoSigner::from_mnemonic(
        "addr_test1...",
        "word1 word2 ... word24",
    )?;

    // 1. Load a compiled .tii protocol and assemble the client through the builder.
    let tx3 = tx3_sdk::tii::Protocol::from_file("./examples/transfer.tii")?
        .client()
        .trp_endpoint("https://trp.example.com")
        .with_profile("preprod")
        .with_party("sender", Party::signer(signer))
        .with_party("receiver", Party::address("addr_test1..."))
        .build()?;

    // 2. Build, resolve, sign, submit, and wait for confirmation
    let status = tx3
        .tx("transfer")?
        .arg("quantity", json!(10_000_000))
        .resolve()
        .await?
        .sign()?
        .submit()
        .await?
        .wait_for_confirmed(PollConfig::default())
        .await?;

    println!("Confirmed at stage: {:?}", status.stage);
    Ok(())
}
```

All fallible validation — TRP endpoint present, profile declared, every bound
party declared — happens inside `build()`, which returns `Error::MissingTrpEndpoint`,
`Error::UnknownProfile`, or `Error::UnknownParty`. Optional setters never return
`Result`, so chains stay fluent up to `build()`. Profile selection is
**builder-only**: there is no profile-switching method on the built client.
Switching profiles requires a new builder.

## Concepts

| SDK Type | Glossary Term | Description |
|---|---|---|
| `tii::Protocol` | TII / Protocol | Loaded `.tii` exposing transactions, parties, profiles. `Protocol::client()` returns a fresh `Tx3ClientBuilder` |
| `Tx3ClientBuilder` | Client builder | Fluent builder seeded by `Protocol::client()` or `Tx3ClientBuilder::from_parts(...)`; absorbs all fallible validation in `build()` |
| `Tx3Client` | Facade | Output of `Tx3ClientBuilder::build()` — owns the deconstructed protocol parts, TRP client, profile, and party bindings |
| `TxBuilder` | Invocation builder | Source-agnostic; collects args, resolves via TRP |
| `Party` | Party | `Party::address(...)` (read-only) or `Party::signer(...)` (signing) |
| `Profile` | Profile | `{ environment, parties }` value baked into the client; embedded by codegen plugins, decomposed from `Protocol` by `from_protocol` |
| `Signer` | Signer | Trait producing a `TxWitness` for a `SignRequest` |
| `SignRequest` | SignRequest | Input passed to `Signer::sign`: `tx_hash_hex` + `tx_cbor_hex` |
| `CardanoSigner` | Cardano Signer | BIP32-Ed25519 signer at `m/1852'/1815'/0'/0/0` |
| `Ed25519Signer` | Ed25519 Signer | Generic raw-key Ed25519 signer |
| `ResolvedTx` | Resolved transaction | Output of `resolve()`, ready for signing |
| `SignedTx` | Signed transaction | Output of `sign()`, ready for submission |
| `SubmittedTx` | Submitted transaction | Output of `submit()`, pollable for status |
| `PollConfig` | Poll configuration | Controls `wait_for_confirmed` / `wait_for_finalized` polling |
| `Error::MissingTrpEndpoint` / `UnknownProfile` / `UnknownParty` / `UnknownTx` | Builder errors | Returned by `build()` and `tx(name)`; named variants of the single `tx3_sdk::Error` enum |

## Advanced usage

### Skipping the runtime `.tii` (codegen flow)

If you've run `trix codegen` to generate typed bindings, your generated `Client`
embeds the per-transaction TIR envelopes and per-profile data at codegen time —
no `.tii` artifact at runtime. Under the hood it seeds the same builder via
`Tx3ClientBuilder::from_parts(transactions, profiles, known_parties)` and routes
typed per-party setters through `with_party_unchecked`. You can also call
`from_parts` directly from hand-written code:

```rust
use std::collections::{HashMap, HashSet};
use tx3_sdk::{Party, Tx3ClientBuilder};

let tx3 = Tx3ClientBuilder::from_parts(transactions, profiles, HashSet::new())
    .trp_endpoint("https://trp.example.com")
    .with_party_unchecked("sender", Party::signer(signer))
    .build()?;
```

### Adding TRP headers

`with_header(key, value)` attaches a header to every TRP request. Combine with
`trp_endpoint` (or `trp(ClientOptions { ... })`) — `with_header` does not supply
an endpoint on its own.

```rust
let tx3 = protocol
    .client()
    .trp_endpoint("https://trp.example.com")
    .with_header("dmtr-api-key", api_key)
    .with_profile("preprod")
    .with_party("sender", Party::signer(signer))
    .build()?;
```

### One-off environment overrides

`with_env_value(key, value)` overlays a single environment value on top of the
selected profile's environment, merged at resolve time (override wins). Useful
for adjusting a network selector or other env without forking a new profile.

```rust
let tx3 = protocol
    .client()
    .trp_endpoint("https://trp.example.com")
    .with_profile("preprod")
    .with_env_value("tax", json!(2_500_000))
    .with_party("sender", Party::signer(signer))
    .build()?;
```

### Low-level TRP client

If you don't want the facade, drive TRP directly:

```rust
use tx3_sdk::trp::{Client, ClientOptions, ResolveParams};

let client = Client::new(ClientOptions {
    endpoint: "https://trp.example.com".to_string(),
    headers: None,
});

// build ResolveParams and call client.resolve(...).await
```

### Custom Signer

Implement the `Signer` trait. `sign` receives a `SignRequest` carrying both the
tx hash and the full tx CBOR; hash-based signers read `tx_hash_hex`, tx-based
signers (e.g. wallet bridges) read `tx_cbor_hex`.

```rust
use tx3_sdk::{SignRequest, Signer};
use tx3_sdk::trp::TxWitness;

struct MySigner { /* ... */ }

impl Signer for MySigner {
    fn address(&self) -> &str { "addr_test1..." }

    fn sign(
        &self,
        request: &SignRequest,
    ) -> Result<TxWitness, Box<dyn std::error::Error + Send + Sync>> {
        // sign request.tx_hash_hex with your key
        unimplemented!()
    }
}
```

### Manual witness attachment

When a witness is produced outside any registered `Signer` — for example by an
external wallet app or a remote signing service — resolve the transaction
first, hand the resolved hash (or full tx CBOR) to the wallet, then attach the
returned witness before `sign()`:

```rust
let resolved = tx3
    .tx("transfer")?
    .arg("quantity", json!(10_000_000))
    .resolve()
    .await?;

// Hand `resolved.hash` (or `resolved.tx_hex`) to the external wallet
// and get back a witness. The wallet needs the resolved tx to sign.
let witness: tx3_sdk::trp::TxWitness = /* sign resolved.hash with external wallet */;

let status = resolved
    .add_witness(witness)
    .sign()?
    .submit()
    .await?
    .wait_for_confirmed(PollConfig::default())
    .await?;
```

`add_witness` may be called any number of times; manual witnesses are appended after registered-signer witnesses in attach order.

## Tx3 protocol compatibility

- **TRP protocol version:** v1beta0
- **TII schema version:** v1beta0

## Testing

- Unit tests are co-located with modules via `#[cfg(test)]`.
- End-to-end (e2e) tests live under `sdk/tests/` and run as Cargo test targets.

```bash
# from rust-sdk/sdk
cargo test --lib
cargo test --test smoke --test codegen --test error_cases --test happy_path
```

## License

Apache-2.0
