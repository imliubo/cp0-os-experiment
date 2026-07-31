#!/usr/bin/env bash
set -euo pipefail

if ! command_line=$(cat /proc/cmdline); then
    echo "cardputerzero-overlay-root: cannot read kernel command line" >&2
    exit 1
fi
if [[ " $command_line " != *" cp0.overlay_root=volatile "* ]]; then
    exit 0
fi

root_type=$(findmnt -n -o FSTYPE /)
if [[ "$root_type" != overlay ]]; then
    echo "cardputerzero-overlay-root: root is $root_type, expected overlay" >&2
    exit 1
fi

lower_options=$(findmnt -n -o OPTIONS /run/cardputerzero-root/lower)
upper_type=$(findmnt -n -o FSTYPE /run/cardputerzero-root/volatile)
upper_options=$(findmnt -n -o OPTIONS /run/cardputerzero-root/volatile)
data_type=$(findmnt -n -o FSTYPE /run/cardputerzero-data)
data_options=$(findmnt -n -o OPTIONS /run/cardputerzero-data)

if [[ ",$lower_options," != *,ro,* ]]; then
    echo "cardputerzero-overlay-root: lower root is not read-only" >&2
    exit 1
fi
if [[ "$upper_type" != tmpfs || ",$upper_options," != *,nodev,* ]]; then
    echo "cardputerzero-overlay-root: volatile upper is not protected tmpfs" >&2
    exit 1
fi
if [[ "$data_type" != ext4 || ",$data_options," != *,rw,* ||
      ",$data_options," != *,nodev,* || ",$data_options," != *,nosuid,* ||
      ",$data_options," != *,noexec,* ]]; then
    echo "cardputerzero-overlay-root: persistent data mount is not protected" >&2
    exit 1
fi
for target in /var/lib/cardputerzero /etc/cardputerzero /etc/ssh \
    /etc/NetworkManager/system-connections /var/lib/NetworkManager \
    /etc/machine-id /var/lib/systemd/random-seed; do
    if ! mountpoint -q "$target"; then
        echo "cardputerzero-overlay-root: persistent bind missing: $target" >&2
        exit 1
    fi
done

echo "cardputerzero-overlay-root: immutable root and persistent data active"
df -h /run/cardputerzero-root/volatile
df -h /run/cardputerzero-data
