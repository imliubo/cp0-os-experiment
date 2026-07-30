#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
target=aarch64-unknown-linux-gnu

cargo build \
    --manifest-path "$repo_root/Cargo.toml" \
    --target "$target" \
    --release \
    -p cp0-appd \
    -p cp0-networkd \
    -p cp0ctl

for binary in cp0-appd cp0-networkd cp0ctl; do
    path="$repo_root/target/$target/release/$binary"
    test -x "$path"
    file "$path" | grep -q 'ELF 64-bit LSB.*ARM aarch64'
    aarch64-linux-gnu-readelf -l "$path" | grep -q 'GNU_RELRO'
    sha256sum "$path"
done
