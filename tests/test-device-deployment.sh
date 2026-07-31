#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
prepare="$repo_root/scripts/prepare-device-deployment.sh"
compositor="$repo_root/scripts/device-install-compositor.sh"
platform="$repo_root/scripts/device-install-app-platform.sh"

bash -n "$prepare"
sh -n "$compositor" "$platform"
grep -q 'wait_active cardputerzero-system-shell.service' "$compositor"
grep -q 'staging/cardputerzero-compositor.service' "$compositor"
grep -q '/etc/systemd/system/cardputerzero-compositor.service' "$compositor"
if grep -q 'wait_active cardputerzero-appd.service' "$compositor"; then
    echo "error: compositor deployment must not depend on appd" >&2
    exit 1
fi
grep -q 'systemctl stop cardputerzero-system-shell.service' "$platform"
grep -q 'cardputerzero-appd.socket cardputerzero-broker.socket' "$platform"
grep -q 'cardputerzero-stored.socket' "$platform"
grep -q 'cp0-stored' "$prepare"
grep -q 'store.conf' "$prepare"
grep -q 'useradd --system --gid cp0-store --groups cp0-control' "$platform"
grep -q 'systemctl start cardputerzero-system-shell.service' "$platform"
grep -q 'device-stability-monitor.sh' "$platform"
grep -q 'output must be below repository target' "$prepare"
grep -q 'rm -f SHA256SUMS' "$prepare"
grep -q 'files/cardputerzero-compositor.service' "$prepare"
