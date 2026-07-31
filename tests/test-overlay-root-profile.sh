#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
files="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
init="$files/overlay-root-initramfs"
hook="$files/cardputerzero-overlay-root.initramfs-hook"
status="$files/overlay-root-status.sh"
service="$files/cardputerzero-overlay-root-status.service"

sh -n "$init" "$hook"
bash -n "$status"
"$init" --test-cmdline \
    'console=tty1 root=/dev/mmcblk0p2 cp0.overlay_root=volatile'
if "$init" --test-cmdline 'console=tty1 root=/dev/mmcblk0p2'; then
    echo "error: overlay root enabled without explicit kernel argument" >&2
    exit 1
fi

grep -q 'manual_add_modules overlay' "$hook"
grep -q 'scripts/init-bottom/cardputerzero-overlay-root' "$hook"
grep -q 'mount -n -o remount,ro' "$init"
grep -q 'mode=0700,size=64M,nr_inodes=16384' "$init"
grep -q 'lowerdir=.*upperdir=.*workdir=' "$init"
grep -q 'panic "CardputerZero volatile root setup failed' "$init"
grep -qx 'ConditionKernelCommandLine=cp0.overlay_root=volatile' "$service"
grep -qx 'ProtectSystem=strict' "$service"
grep -q 'cardputerzero-overlay-root.initramfs-hook' "$stage"
grep -q 'cardputerzero-overlay-root-status.service rpi-resize.service' "$stage"
grep -q 'update-initramfs -u -k all' "$stage"
grep -q '^kernel.kptr_restrict=2$' "$stage"
grep -q '^kernel.unprivileged_bpf_disabled=1$' "$stage"
grep -q '^tmpfs /tmp tmpfs nodev,nosuid,noatime,mode=1777,size=32M' "$stage"

if grep -Eq 'for token in .*cp0\.overlay_root|sed .*cp0\.overlay_root' "$stage"; then
    echo "error: volatile overlay root must remain opt-in" >&2
    exit 1
fi
