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
if grep -q 'systemctl.*wait command' "$platform"; then
    echo 'error: app-platform installer rejects supported systemctl versions using an unused wait subcommand' >&2
    exit 1
fi
grep -q 'cardputerzero-appd.socket cardputerzero-broker.socket' "$platform"
grep -q 'cardputerzero-stored.socket' "$platform"
grep -q 'cardputerzero-audiod.socket cardputerzero-camerad.socket' "$platform"
grep -q 'cardputerzero-radiod.socket cardputerzero-storaged.socket' "$platform"
grep -q 'cp0-stored' "$prepare"
grep -q 'cp0-displayd' "$prepare"
grep -q 'cp0-connectivityd' "$prepare"
grep -q 'cardputerzero-connectivityd.socket' "$platform"
grep -q 'cardputerzero-displayd.socket' "$platform"
grep -q 'for group in cp0-control cp0-display-control cp0-audio-control cp0-connectivity-control' "$compositor"
grep -q 'usermod -a -G "$group" cp0-shell' "$compositor"
grep -q 'usermod -a -G cp0-display-control cp0-shell' "$platform"
grep -q 'usermod -a -G cp0-connectivity-control cp0-shell' "$platform"
grep -q 'store.conf' "$prepare"
grep -q 'device-policy.json' "$prepare"
grep -q '/etc/cardputerzero/device-policy.json' "$platform"
grep -q 'useradd --system --gid cp0-store --groups cp0-control' "$platform"
grep -q 'systemctl start cardputerzero-system-shell.service' "$platform"
grep -q 'device-stability-monitor.sh' "$platform"
grep -q 'device-capability-acceptance.sh' "$platform"
grep -q 'device-capability-acceptance.sh' "$prepare"
grep -q 'device-factory-acceptance.sh' "$platform"
grep -q 'device-factory-acceptance.sh' "$prepare"
grep -q 'device-smoke.sh' "$platform"
grep -q 'device-smoke.sh' "$prepare"
grep -q '/usr/libexec/cardputerzero/device-smoke.sh' "$platform"
grep -q '/usr/libexec/cardputerzero/device-factory-acceptance' "$platform"
grep -q 'device-performance-acceptance.sh' "$platform"
grep -q 'device-performance-acceptance.sh' "$prepare"
grep -q 'device-store-acceptance.sh' "$platform"
grep -q 'device-store-acceptance.sh' "$prepare"
grep -q 'output must be below repository target' "$prepare"
grep -q 'rm -f SHA256SUMS' "$prepare"
grep -q 'files/cardputerzero-compositor.service' "$prepare"
grep -q 'files/cardputerzero-recovery-console.service' "$prepare"
grep -q 'files/cardputerzero-display-generator' "$prepare"
grep -q '/usr/lib/systemd/system-generators/cardputerzero-display-generator' "$compositor"
grep -q 'disable recovery mode before compositor deployment' "$compositor"
