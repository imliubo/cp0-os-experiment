#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
recovery="$repo_root/scripts/device-core-recovery.sh"
monitor="$repo_root/scripts/device-stability-monitor.sh"
factory="$repo_root/scripts/device-factory-acceptance.sh"
performance="$repo_root/scripts/device-performance-acceptance.sh"
capability="$repo_root/scripts/device-capability-acceptance.sh"
support="$repo_root/scripts/device-support-bundle.sh"
build="$repo_root/image/build-image.sh"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"

bash -n "$recovery" "$monitor" "$factory" "$performance" "$capability" "$support"
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

grep -q 'result_root=/run/cardputerzero-factory' "$factory"
grep -q '/usr/libexec/cardputerzero/device-smoke.sh' "$factory"
grep -q 'BASH_REMATCH\[1\]' "$factory"
grep -q 'cp0.overlay_root=volatile' "$factory"
grep -q '/dev/mmcblk0p3' "$factory"
grep -q 'data-filesystem-expanded' "$factory"
grep -q 'blockdev --getsize64 /dev/mmcblk0p3' "$factory"
grep -q 'cp0-data-layout-v1' "$factory"
grep -q '\$restarts != 0' "$factory"
grep -q '/usr/bin/cp0ctl app ping' "$factory"
grep -q 'systemctl --failed' "$factory"
grep -q 'default-mode:' "$factory"
grep -q 'FAILED failures=' "$factory"
if grep -Eq '(^|[[:space:]])(dd|mkfs|mount|umount|reboot|poweroff)([[:space:]]|$)' \
    "$factory"; then
    echo "error: factory acceptance contains a destructive or mount-mutating command" >&2
    exit 1
fi

grep -q 'result_root=/run/cardputerzero-capability' "$capability"
grep -q 'cardputerzero-stability-acceptance.service' "$capability"
grep -q -- '--persistence-only' "$capability"
grep -q 'CP0_AUDIO_OBSERVED' "$capability"
grep -q '/proc/sys/kernel/random/boot_id' "$capability"
if grep -Eq '(^|[[:space:]])(dd|mkfs|mount|umount|reboot|poweroff)([[:space:]]|$)' \
    "$capability"; then
    echo "error: capability acceptance contains a destructive or boot-mutating command" >&2
    exit 1
fi
if grep -Eq '(^|[[:space:]])rm([[:space:]]|$)' "$factory"; then
    echo "error: factory acceptance must never delete result data" >&2
    exit 1
fi

grep -q 'result_root=/run/cardputerzero-performance' "$performance"
grep -q 'cardputerzero-stability-acceptance.service' "$performance"
grep -q 'FinishTimestampMonotonic' "$performance"
grep -q 'ActiveEnterTimestampMonotonic' "$performance"
grep -q 'CPUUsageNSec' "$performance"
grep -q 'MemoryCurrent' "$performance"
grep -q '/sys/block/mmcblk0/stat' "$performance"
grep -q -- "-name 'bq27220-\*'" "$performance"
grep -q 'max_boot_ready_ms=35000' "$performance"
grep -q 'max_idle_used_bytes=\$((180 \* 1024 \* 1024))' "$performance"
grep -q 'estimated_average=.*record-only' "$performance"
if grep -Eq \
    'systemctl[[:space:]]+(start|stop|restart|kill)|cp0ctl[[:space:]]+app[[:space:]]+(start|stop)|(^|[[:space:]])(dd|mkfs|mount|umount|reboot|poweroff)([[:space:]]|$)' \
    "$performance"; then
    echo "error: performance acceptance contains a state-mutating command" >&2
    exit 1
fi
if grep -Eq '(^|[[:space:]])rm([[:space:]]|$)' "$performance"; then
    echo "error: performance acceptance must never delete result data" >&2
    exit 1
fi

grep -q 'result_root=/run/cardputerzero-support' "$support"
grep -q 'include_journal=0' "$support"
grep -q -- '--include-journal' "$support"
grep -q 'sensitive-journal.txt' "$support"
grep -q 'journal_included=' "$support"
grep -q 'never uploaded automatically' "$support"
grep -q 'chmod 0600' "$support"
grep -q 'tar --sort=name --owner=0 --group=0 --numeric-owner' "$support"
if grep -Eq \
    'cat.*(/etc/machine-id|/etc/hostname|ssh_host)|nmcli.*connection|ip[[:space:]].*address|/var/lib/cardputerzero/(apps|data|documents)' \
    "$support"; then
    echo "error: default support bundle reads a forbidden identifier or user-data path" >&2
    exit 1
fi
if grep -Eq 'curl|wget|scp|rsync|nc[[:space:]]|socat' "$support"; then
    echo "error: support bundle must not contain an upload path" >&2
    exit 1
fi

grep -q 'device-core-recovery.sh' "$build"
grep -q 'device-capability-acceptance.sh' "$build"
grep -q 'device-factory-acceptance.sh' "$build"
grep -q 'device-performance-acceptance.sh' "$build"
grep -q 'device-stability-monitor.sh' "$build"
grep -q 'device-support-bundle.sh' "$build"
grep -q '/usr/libexec/cardputerzero/device-core-recovery' "$stage"
grep -q '/usr/libexec/cardputerzero/device-capability-acceptance' "$stage"
grep -q '/usr/libexec/cardputerzero/device-factory-acceptance' "$stage"
grep -q '/usr/libexec/cardputerzero/device-performance-acceptance' "$stage"
grep -q '/usr/libexec/cardputerzero/device-stability-monitor' "$stage"
grep -q '/usr/libexec/cardputerzero/device-support-bundle' "$stage"
