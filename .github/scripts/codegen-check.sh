#!/usr/bin/env bash
#
# CI artifact — not part of the SDK.
#
# Renders tx3c's built-in `rust-client` template against the shared transfer
# fixture and verifies the result:
#   - the expected public surface is generated, and
#   - the rendered crate compiles.
#
# The template ships inside tx3c and pins a published `tx3-sdk` range. The
# rendered crate is compiled against the in-repo `sdk/` crate via a
# `[patch.crates-io]` override, so an SDK change that would break the clients
# the current tx3c generates fails here, before the SDK is released. The
# generated `Cargo.toml` still carries the real version requirement; only the
# source is redirected.
#
# Requires `tx3c` (0.24.0 or later, which ships the built-in templates) and `cargo` on PATH.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
gen="$(mktemp -d)"
gen_complex="$(mktemp -d)"
trap 'rm -rf "$gen" "$gen_complex"' EXIT

tx3c codegen \
  --tii "$repo_root/sdk/tests/fixtures/transfer.tii" \
  --template rust-client \
  --output "$gen"

for f in lib.rs Cargo.toml; do
  test -f "$gen/$f" || { echo "missing generated file: $f"; exit 1; }
done

# Public surface of the generated lifecycle client.
for sym in \
  'pub const TARGET_TII_VERSION' \
  'pub static TRANSFER_TIR: LazyLock<TirEnvelope>' \
  'pub enum Profile' \
  'Preprod,' \
  'pub struct TransferParams' \
  'pub struct Client' \
  'pub fn new(options: ClientOptions, profile: Profile)' \
  'pub fn with_sender(' \
  'pub fn with_receiver(' \
  'pub fn with_middleman(' \
  'pub fn transfer(&self, args: TransferParams) -> TxBuilder'; do
  grep -qF "$sym" "$gen/lib.rs" || { echo "generated lib.rs missing: $sym"; exit 1; }
done

# A generic with_party(name, party) MUST NOT leak through the typed wrapper.
if grep -qE 'pub fn with_party' "$gen/lib.rs"; then
  echo "generated lib.rs exposes generic with_party — should be typed per party"
  exit 1
fi

# The wrapper builder layer is gone — no ClientBuilder, no Client::builder().
for forbidden in \
  'pub struct ClientBuilder' \
  'pub fn builder(' \
  'pub fn with_profile' \
  'pub fn with_env_value' \
  'pub fn with_header'; do
  if grep -qF "$forbidden" "$gen/lib.rs"; then
    echo "generated lib.rs exposes removed surface: $forbidden"
    exit 1
  fi
done

# Compile against the SDK in this checkout, so an unreleased SDK change is
# checked against the client the current tx3c generates.
cat >> "$gen/Cargo.toml" <<EOF

[patch.crates-io]
tx3-sdk = { path = "$repo_root/sdk" }
EOF

cargo check --manifest-path "$gen/Cargo.toml"

# The transfer fixture is deliberately plain: no custom types, no UtxoRef
# params. Render `complex.tii` as well, which carries both. It is the only
# fixture whose embedded schemas contain local `"$ref":"#/components/schemas/…"`
# refs, and the only one that exercises the `UtxoRef`/`Address` core imports —
# the two things a transfer-only check cannot see.
tx3c codegen \
  --tii "$repo_root/sdk/tests/fixtures/complex.tii" \
  --template rust-client \
  --output "$gen_complex"

# Local component refs must survive into the generated source. A raw string
# delimited `r#"…"#` is closed early by the `"#` in such a ref, which produces
# a file that parses as garbage rather than one that is merely wrong.
grep -qF '#/components/schemas/' "$gen_complex/lib.rs" || {
  echo "generated lib.rs lost its local component refs"
  exit 1
}
grep -qF 'UtxoRef' "$gen_complex/lib.rs" || {
  echo "generated lib.rs missing the UtxoRef core import"
  exit 1
}

cat >> "$gen_complex/Cargo.toml" <<EOF

[patch.crates-io]
tx3-sdk = { path = "$repo_root/sdk" }
EOF

cargo check --manifest-path "$gen_complex/Cargo.toml"

echo "codegen check passed"
