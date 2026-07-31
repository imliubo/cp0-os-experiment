#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output=${1:-$repo_root/target/device-deployment}

case "$output" in
    "$repo_root"/target/*) ;;
    *)
        echo "error: deployment output must be below repository target/" >&2
        exit 2
        ;;
esac

runtime="$repo_root/target/app-runtime-aarch64/cardputerzero-app-runtime"
compositor="$repo_root/target/compositor-aarch64"
release="$repo_root/target/aarch64-unknown-linux-gnu/release"
hello="$repo_root/target/apps/dev.cardputerzero.hello/0.1.0"

required=(
    "$runtime"
    "$compositor/cardputerzero-system-shell"
    "$compositor/cardputerzero-policy.so"
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-compositor.service"
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-display-retry.service"
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/retry-display-once.sh"
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-display-generator"
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-recovery-console.service"
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-system-shell.service"
    "$hello/app.json"
    "$hello/bin/hello-card.wasm"
    "$release/cp0-appd"
    "$release/cp0-audiod"
    "$release/cp0-camerad"
    "$release/cp0-documentd"
    "$release/cp0-gpiod"
    "$release/cp0-networkd"
    "$release/cp0-radiod"
    "$release/cp0-storaged"
    "$release/cp0-stored"
    "$release/cp0ctl"
)
for file in "${required[@]}"; do
    if [[ ! -f $file || -L $file ]]; then
        echo "error: required deployment artifact missing or symbolic: $file" >&2
        exit 1
    fi
done

mkdir -p "$output"
install -m 0755 "$runtime" "$output/cardputerzero-app-runtime"
install -m 0755 \
    "$compositor/cardputerzero-system-shell" \
    "$compositor/cardputerzero-policy.so" \
    "$release/cp0-appd" \
    "$release/cp0-audiod" \
    "$release/cp0-camerad" \
    "$release/cp0-documentd" \
    "$release/cp0-gpiod" \
    "$release/cp0-networkd" \
    "$release/cp0-radiod" \
    "$release/cp0-storaged" \
    "$release/cp0-stored" \
    "$release/cp0ctl" \
    "$repo_root/scripts/device-install-compositor.sh" \
    "$repo_root/scripts/device-install-app-platform.sh" \
    "$repo_root/scripts/device-capability-acceptance.sh" \
    "$repo_root/scripts/device-core-recovery.sh" \
    "$repo_root/scripts/device-stability-monitor.sh" \
    "$repo_root/scripts/device-store-acceptance.sh" \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-display-generator" \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/retry-display-once.sh" \
    "$output/"
install -m 0644 "$hello/app.json" "$output/app.json"
install -m 0644 "$hello/bin/hello-card.wasm" "$output/hello-card.wasm"
install -m 0644 \
    "$repo_root/appd/systemd/"* \
    "$repo_root/appd/lora.conf" \
    "$repo_root/appd/store.conf" \
    "$repo_root/appd/device-policy.json" \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-compositor.service" \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-display-retry.service" \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-recovery-console.service" \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-system-shell.service" \
    "$output/"

(
    cd "$output"
    rm -f SHA256SUMS
    shasum -a 256 -- * >SHA256SUMS
)
echo "$output"
