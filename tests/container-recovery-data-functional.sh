#!/bin/bash
set -euo pipefail

if (($# != 2)); then
    echo "usage: $0 CP0_RECOVERY_BINARY NEW_WORK_ROOT" >&2
    exit 2
fi
binary=$1
work_root=$2
if [[ $work_root != /* ]] || [[ -e $work_root ]]; then
    echo "error: functional work root must be a new absolute path" >&2
    exit 2
fi

source_root="$work_root/source"
restore_root="$work_root/restored"
bundle="$work_root/round-trip.cp0backup"
install -d -m 0700 \
    "$source_root/cardputerzero/private/dev.example" \
    "$source_root/etc-cardputerzero" \
    "$source_root/extrausers" \
    "$source_root/home" \
    "$source_root/network-connections" \
    "$source_root/network-state" \
    "$source_root/ssh" \
    "$restore_root"
printf 'cp0-data-layout-v2\n' >"$source_root/layout-version"
for database in passwd group shadow gshadow; do
    : >"$source_root/extrausers/$database"
done
printf 'product\n' >"$source_root/etc-cardputerzero/image-profile"
printf '0123456789abcdef0123456789abcdef\n' >"$source_root/machine-id"
printf 'random-seed-fixture' >"$source_root/random-seed"
printf 'private-value' \
    >"$source_root/cardputerzero/private/dev.example/value"
chmod 0600 \
    "$source_root/layout-version" \
    "$source_root/etc-cardputerzero/image-profile" \
    "$source_root/machine-id" \
    "$source_root/random-seed" \
    "$source_root/cardputerzero/private/dev.example/value"

"$binary" backup "$source_root" "$bundle"
"$binary" verify "$bundle"
"$binary" restore "$bundle" "$restore_root"
cmp \
    "$source_root/cardputerzero/private/dev.example/value" \
    "$restore_root/cardputerzero/private/dev.example/value"
test "$(stat -c %a "$restore_root/cardputerzero/private/dev.example/value")" = 600
test "$(stat -c %U "$restore_root/cardputerzero/private/dev.example/value")" = root
echo "PASS Linux ARM64 cp0 backup v1 functional round trip"
