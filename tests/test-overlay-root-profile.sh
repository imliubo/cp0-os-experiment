#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
files="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
init="$files/overlay-root-initramfs"
grow="$files/data-grow-initramfs"
hook="$files/cardputerzero-overlay-root.initramfs-hook"
status="$files/overlay-root-status.sh"
service="$files/cardputerzero-overlay-root-status.service"
export_prerun="$repo_root/image/pi-gen/export-image-prerun.sh"

sh -n "$init" "$grow" "$hook"
bash -n "$export_prerun"
bash -n "$status"
"$init" prereqs
"$grow" prereqs
"$init" --test-cmdline \
    'console=tty1 root=/dev/mmcblk0p2 cp0.overlay_root=volatile'
if "$init" --test-cmdline 'console=tty1 root=/dev/mmcblk0p2'; then
    echo "error: overlay root enabled without explicit kernel argument" >&2
    exit 1
fi

grep -q 'manual_add_modules overlay' "$hook"
grep -q 'manual_add_modules ext4' "$hook"
if grep -q 'scripts/local-premount/cardputerzero-data-grow\|scripts/init-bottom/cardputerzero-overlay-root' "$hook"; then
    echo "error: initramfs boot scripts copied by hooks are omitted from ORDER" >&2
    exit 1
fi
grep -q 'mount -n -o remount,ro' "$init"
grep -q 'mode=0700,size=64M,nr_inodes=16384' "$init"
grep -q 'lowerdir=.*upperdir=.*workdir=' "$init"
grep -q 'panic "CardputerZero volatile root setup failed' "$init"
grep -q 'LABEL=cp0-data' "$init"
grep -q 'persistent image profile is not product' "$init"
grep -q 'bind_directory "$data/cardputerzero"' "$init"
grep -q 'bind_file "$data/machine-id"' "$init"
grep -q 'partition_number.*-eq 3' "$grow"
grep -q 'data partition is not last' "$grow"
grep -q 'resize2fs "$data_device"' "$grow"
grep -qx 'ConditionKernelCommandLine=cp0.overlay_root=volatile' "$service"
grep -qx 'ProtectSystem=strict' "$service"
grep -q 'cannot read kernel command line' "$status"
if grep -qx 'ProcSubset=pid' "$service"; then
    echo "error: overlay status service cannot hide /proc/cmdline" >&2
    exit 1
fi
grep -q 'cardputerzero-overlay-root.initramfs-hook' "$stage"
grep -q '/etc/initramfs-tools/scripts/init-bottom/cardputerzero-overlay-root' "$stage"
grep -q '/etc/initramfs-tools/scripts/local-premount/cardputerzero-data-grow' "$stage"
grep -q 'cardputerzero-overlay-root-status.service' "$stage"
grep -q 'rpi-resize.service 2>/dev/null' "$stage"
grep -q 'update-initramfs -u -k all' "$stage"
grep -q '^kernel.kptr_restrict=2$' "$stage"
grep -q '^kernel.unprivileged_bpf_disabled=1$' "$stage"
grep -q '^tmpfs /tmp tmpfs nodev,nosuid,noatime,mode=1777,size=32M' "$stage"
grep -q '^tmpfs /var/tmp tmpfs nodev,nosuid,noatime,mode=1777,size=128M' "$stage"

grep -q 'cp0.overlay_root=volatile' "$stage"
grep -q "s/.*resize" "$stage"
grep -q 'DATA_SIZE=.*256' "$export_prerun"
grep -q 'DATA_DEV=.*p3' "$export_prerun"
grep -q 'mkfs.ext4 -L cp0-data' "$export_prerun"
grep -q 'cp0-data-layout-v1' "$export_prerun"
if grep -q 'cardputerzero-overlay-root-status.service rpi-resize.service' "$stage"; then
    echo "error: root resize must remain disabled with a third partition" >&2
    exit 1
fi
