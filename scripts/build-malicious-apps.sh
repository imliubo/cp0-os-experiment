#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output_root="$repo_root/target/malicious"
mkdir -p "$output_root"

for name in memory-hog ambient-authority; do
    fixture="$repo_root/tests/malicious/$name"
    build_root="$repo_root/target/malicious-build/$name"
    artifact_name=${name//-/_}
    output="$output_root/$name.wasm"
    CARGO_TARGET_DIR="$build_root" cargo build \
        --manifest-path "$fixture/Cargo.toml" \
        --target wasm32-unknown-unknown \
        --release
    install -m 0644 \
        "$build_root/wasm32-unknown-unknown/release/cp0_$artifact_name.wasm" \
        "$output"
    test -s "$output"
    sha256sum "$output"
done
