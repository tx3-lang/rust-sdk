# TX3 Rust SDK

Ergonomic Rust SDK for interacting with UTxO-based blockchains using the Tx3 toolkit.

## What is TX3?

TX3 is a domain-specific language (DSL) and protocol suite for defining and executing UTxO-based blockchain transactions in a declarative, type-safe manner. It abstracts the complexity of UTXO management and transaction construction while maintaining flexibility for complex DeFi and smart contract interactions.

## Quick start

Add the SDK to your project:

```toml
[dependencies]
tx3-sdk = "0.9"
serde_json = "1"
```

### Full lifecycle (recommended facade)

```rust
use serde_json::json;
use tx3_sdk::trp::{Client, ClientOptions};
use tx3_sdk::{CardanoSigner, Party, PollConfig, Tx3Client};

let signer = CardanoSigner::from_mnemonic(
    "addr_test1...",
    "word1 word2 ... word24",
)?;

let protocol = tx3_sdk::tii::Protocol::from_file("./examples/transfer.tii")?;

let trp = Client::new(ClientOptions {
    endpoint: "https://trp.example.com".to_string(),
    headers: None,
});

let tx3 = Tx3Client::new(protocol, trp)
    .with_profile("preprod")
    .with_party("sender", Party::signer(signer))
    .with_party("receiver", Party::address("addr_test1..."));

// this will call the TRP to compile the intent into
// a fully-defined transaction.
let invocation = tx3
    .tx("transfer")
    .arg("quantity", json!(10_000_000))
    .resolve()
    .await?;

// this will use the configured parties (those which are `signers`) to
// sign the transaction.
let signed = invocation.sign()?;

// this will submit the signed payload to the chain using the TRP server.
let submitted = signed.submit().await?;

// this will poll the submitted tx waiting for confirmation that is has
// reached the chain.
let status = submitted
    .wait_for_confirmed(PollConfig::default())
    .await?;

println!("Confirmed at stage: {:?}", status.stage);
```

## Concepts

### Protocols
Protocols are defined in TII files and loaded via `tii::Protocol`.

```rust
let protocol = tx3_sdk::tii::Protocol::from_file("./examples/transfer.tii")?;
```

### Parties
Parties are declared in the protocol and attached to the client:

- `Party::address(...)` for read-only parties (address only)
- `Party::signer(...)` for signing parties (address comes from signer)

Parties are injected into invocation args by name. You can still override any param
explicitly with `.arg(...)` if needed.

### Signers
Signers produce TRP witnesses from a tx hash.

- `CardanoSigner` is Cardano-specific and derives keys using the
  Cardano path `m/1852'/1815'/0'/0/0`.
- `Ed25519Signer` is a generic raw-key signer (address required at setup).

```rust
use tx3_sdk::CardanoSigner;

let signer = CardanoSigner::from_mnemonic(
    "addr_test1...",
    "word1 word2 ...",
)?;
```

### Profiles
Profiles are set at the client level and applied to all invocations:

```rust
let tx3 = Tx3Client::new(protocol, trp).with_profile("preprod");
```

### Waiting for status
There are two wait modes:

- `wait_for_confirmed` (default for most apps)
- `wait_for_finalized` (stronger finality)

```rust
let confirmed = submitted.wait_for_confirmed(PollConfig::default()).await?;
let finalized = submitted.wait_for_finalized(PollConfig::default()).await?;
```

## Advanced: low-level TRP client

If you need full control, use the low-level TRP client directly:

```rust
use tx3_sdk::trp::{Client, ClientOptions, ResolveParams};

let client = Client::new(ClientOptions {
    endpoint: "https://trp.example.com".to_string(),
    headers: None,
});

// ... build ResolveParams and call client.resolve(...)
```

## Testing

- Unit tests are co-located with modules via `#[cfg(test)]`.
- Integration tests are under `sdk/tests/` and run as Cargo integration targets.

```bash
# from rust-sdk/sdk
cargo test --lib
cargo test --test smoke --test codegen --test error_cases --test happy_path
```

## License

Apache-2.0
