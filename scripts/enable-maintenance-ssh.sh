#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
    echo "usage: $0 BOOT_MOUNT ED25519_PUBLIC_KEY" >&2
    exit 2
fi

boot_mount=$1
public_key=$2
[ -d "$boot_mount" ] && [ ! -L "$boot_mount" ] || {
    echo "error: boot mount is not a directory" >&2
    exit 1
}
[ -f "$boot_mount/config.txt" ] && [ -f "$boot_mount/cmdline.txt" ] || {
    echo "error: target does not look like a Raspberry Pi boot partition" >&2
    exit 1
}
[ -f "$public_key" ] && [ ! -L "$public_key" ] || {
    echo "error: public key is not a regular file" >&2
    exit 1
}

key_type=$(awk 'NR == 1 { print $1 } END { if (NR != 1) exit 1 }' "$public_key")
[ "$key_type" = ssh-ed25519 ] || {
    echo "error: exactly one ED25519 public key is required" >&2
    exit 1
}
ssh-keygen -l -f "$public_key" >/dev/null

key_target=$boot_mount/cp0-maintenance.authorized_key
marker_target=$boot_mount/cp0-maintenance.enable
umask 077
tr -d '\r' <"$public_key" >"$key_target.new"
printf '%s\n' cp0-maintenance-v1 >"$marker_target.new"
mv -f "$key_target.new" "$key_target"
mv -f "$marker_target.new" "$marker_target"
sync

echo "One-boot maintenance SSH is armed for root public-key login."
echo "The device consumes both files during the next boot."
