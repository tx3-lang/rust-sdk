#!/usr/bin/env bash
#
# CI artifact — not part of the SDK.
#
# Renders the .trix/client-lib codegen plugin against the shared transfer
# fixture and verifies the result. The subject under test is the Handlebars
# templates + tx3c integration, not the SDK runtime.
#
# Steps: invoke `tx3c codegen`, assert the expected files exist, smoke-check
# the generated surface, and compile the output against this repo's SDK.
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

for sym in \
  'TARGET_TII_VERSION' \
  'pub struct TransferParams' \
  'pub fn transfer_tir' \
  'pub struct Client' \
  'pub async fn transfer'; do
  grep -qF "$sym" "$gen/lib.rs" || { echo "generated lib.rs missing: $sym"; exit 1; }
done

# Check the rendered crate against this repo's SDK, not a published release.
printf '\n[patch.crates-io]\ntx3-sdk = { path = "%s/sdk" }\n' "$repo_root" >> "$gen/Cargo.toml"
cargo check --manifest-path "$gen/Cargo.toml"

echo "codegen check passed"
