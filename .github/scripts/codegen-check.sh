#!/usr/bin/env bash
#
# CI artifact — not part of the SDK.
#
# Renders the .trix/client-lib codegen plugin against the shared transfer
# fixture and verifies the result the way a consumer would: the rendered crate
# resolves the published `tx3-sdk` from crates.io at the version its generated
# Cargo.toml pins — no patches or path overrides.
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

cargo check --manifest-path "$gen/Cargo.toml"

echo "codegen check passed"
