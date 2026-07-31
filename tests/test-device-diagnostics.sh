#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
recovery="$repo_root/scripts/device-core-recovery.sh"
monitor="$repo_root/scripts/device-stability-monitor.sh"
build="$repo_root/image/build-image.sh"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"

bash -n "$recovery" "$monitor"
grep -q 'stop the foreground application before recovery testing' "$recovery"
grep -q 'systemctl kill --kill-whom=main --signal=KILL' "$recovery"
grep -q 'cardputerzero-compositor.service' "$recovery"
grep -q 'cardputerzero-system-shell.service' "$recovery"
grep -q 'cardputerzero-appd.service' "$recovery"
grep -q '/usr/bin/cp0ctl app ping' "$recovery"
grep -q '/run/cardputerzero-broker/runtime.sock' "$recovery"
grep -q 'stored=cardputerzero-stored.service' "$recovery"
grep -q 'systemctl is-active --quiet.*stored' "$recovery"
grep -q '/run/cardputerzero-store/control.sock' "$recovery"
if grep -Eq 'kill.*cardputerzero-|pkill|killall' "$recovery"; then
    echo "error: recovery script uses process-name or broad kill" >&2
    exit 1
fi

grep -q '/run/cardputerzero-stability' "$monitor"
grep -q 'MemoryCurrent' "$monitor"
grep -q 'NRestarts' "$monitor"
grep -q '/usr/bin/cp0ctl app ping' "$monitor"
grep -q 'memory-growth' "$monitor"
grep -q '/sys/block/mmcblk0/stat' "$monitor"
grep -q 'sd_write_bytes' "$monitor"
grep -q 'maximum_sd_write_bytes' "$monitor"
grep -q 'store_unit=cardputerzero-stored.service' "$monitor"
grep -q 'systemctl is-active --quiet.*store_unit' "$monitor"
grep -q '/run/cardputerzero-store/control.sock' "$monitor"
if grep -Eq '(^|[[:space:]])rm([[:space:]]|$)' "$monitor"; then
    echo "error: stability monitor must never delete result data" >&2
    exit 1
fi

grep -q 'device-core-recovery.sh' "$build"
grep -q 'device-stability-monitor.sh' "$build"
grep -q '/usr/libexec/cardputerzero/device-core-recovery' "$stage"
grep -q '/usr/libexec/cardputerzero/device-stability-monitor' "$stage"
