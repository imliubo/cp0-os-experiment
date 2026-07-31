#!/usr/bin/env bash
set -euo pipefail

if [[ " $(cat /proc/cmdline) " != *" cp0.overlay_root=volatile "* ]]; then
    exit 0
fi

root_type=$(findmnt -n -o FSTYPE /)
lower_options=$(findmnt -n -o OPTIONS /run/cardputerzero-root/lower)
upper_type=$(findmnt -n -o FSTYPE /run/cardputerzero-root/volatile)
upper_options=$(findmnt -n -o OPTIONS /run/cardputerzero-root/volatile)

if [[ "$root_type" != overlay ]]; then
    echo "cardputerzero-overlay-root: root is $root_type, expected overlay" >&2
    exit 1
fi
if [[ ",$lower_options," != *,ro,* ]]; then
    echo "cardputerzero-overlay-root: lower root is not read-only" >&2
    exit 1
fi
if [[ "$upper_type" != tmpfs || ",$upper_options," != *,nodev,* ]]; then
    echo "cardputerzero-overlay-root: volatile upper is not protected tmpfs" >&2
    exit 1
fi

echo "cardputerzero-overlay-root: read-only lower and volatile upper active"
df -h /run/cardputerzero-root/volatile
