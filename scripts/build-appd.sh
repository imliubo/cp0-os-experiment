#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
target=aarch64-unknown-linux-gnu

cargo build \
    --manifest-path "$repo_root/Cargo.toml" \
    --target "$target" \
    --release \
    -p cp0-appd \
    -p cp0-audiod \
    -p cp0-camerad \
    -p cp0-connectivityd \
    -p cp0-displayd \
    -p cp0-devd \
    -p cp0-documentd \
    -p cp0-gpiod \
    -p cp0-networkd \
    -p cp0-powerd \
    -p cp0-provisiond \
    -p cp0-radiod \
    -p cp0-recovery \
    -p cp0-storaged \
    -p cp0-stored \
    -p cp0-usb-mediad \
    -p cp0ctl

for binary in cp0-appd cp0-audiod cp0-camerad cp0-connectivityd cp0-displayd cp0-devd cp0-documentd cp0-gpiod cp0-networkd cp0-powerd cp0-provisiond cp0-radiod cp0-recovery cp0-storaged cp0-stored cp0-usb-mediad cp0ctl; do
    path="$repo_root/target/$target/release/$binary"
    test -x "$path"
    file "$path" | grep -q 'ELF 64-bit LSB.*ARM aarch64'
    aarch64-linux-gnu-readelf -l "$path" | grep -q 'GNU_RELRO'
    sha256sum "$path"
done
