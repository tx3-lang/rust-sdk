#!/usr/bin/env bash
#
# CI artifact — not part of the SDK.
#
# Renders the .trix/client-lib codegen plugin against the shared transfer
# fixture and verifies the result:
#   - the expected public surface is generated, and
#   - the rendered crate compiles.
#
# The template tracks the `tx3-sdk` in this repo, which may be ahead of the
# crate published on crates.io — a PR lands the SDK and the template changes
# together, before the release. So the rendered crate is compiled against the
# in-repo `sdk/` crate via a `[patch.crates-io]` override: this verifies the
# template and the SDK in the same checkout stay consistent. The generated
# `Cargo.toml` still carries the real version requirement; only the source is
# redirected.
#
# Requires `tx3c` and `cargo` on PATH.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
gen="$(mktemp -d)"
trap 'rm -rf "$gen"' EXIT

tx3c codegen \
  --tii "$repo_root/sdk/tests/fixtures/transfer.tii" \
  --template "$repo_root/.trix/client-lib" \
  --output "$gen"

for f in lib.rs Cargo.toml; do
  test -f "$gen/$f" || { echo "missing generated file: $f"; exit 1; }
done

# Public surface of the generated lifecycle client.
for sym in \
  'pub const TARGET_TII_VERSION' \
  'pub struct TransferParams' \
  'pub struct Client' \
  'pub struct ClientBuilder' \
  'pub fn new(options: ClientOptions)' \
  'pub fn builder(options: ClientOptions) -> ClientBuilder' \
  'pub fn with_profile' \
  'pub fn with_sender(' \
  'pub fn with_receiver(' \
  'pub fn with_middleman(' \
  'pub fn with_env_value' \
  'pub fn build(self) -> Client' \
  'pub fn transfer(&self, args: TransferParams) -> TxBuilder'; do
  grep -qF "$sym" "$gen/lib.rs" || { echo "generated lib.rs missing: $sym"; exit 1; }
done

# A generic with_party(name, party) MUST NOT leak through the typed wrapper.
if grep -qE 'pub fn with_party' "$gen/lib.rs"; then
  echo "generated lib.rs exposes generic with_party — should be typed per party"
  exit 1
fi

# Compile against the SDK in this checkout — the template may require a
# `tx3-sdk` version not yet published to crates.io.
cat >> "$gen/Cargo.toml" <<EOF

[patch.crates-io]
tx3-sdk = { path = "$repo_root/sdk" }
EOF

cargo check --manifest-path "$gen/Cargo.toml"

echo "codegen check passed"
