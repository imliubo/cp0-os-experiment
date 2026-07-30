#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
fixture="$repo_root/tests/malicious/memory-hog"
output="$repo_root/target/malicious/memory-hog.wasm"

cargo build \
    --manifest-path "$fixture/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release

mkdir -p "$(dirname "$output")"
install -m 0644 \
    "$fixture/target/wasm32-unknown-unknown/release/cp0_memory_hog.wasm" \
    "$output"
test -s "$output"
sha256sum "$output"
