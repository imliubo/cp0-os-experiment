#!/bin/sh
set -eu

boot_dir=${CP0_MAINTENANCE_BOOT_DIR:-/boot/firmware}
enable_file=$boot_dir/cp0-maintenance.enable
key_file=$boot_dir/cp0-maintenance.authorized_key
status_file=$boot_dir/cp0-maintenance.status
runtime_dir=${CP0_MAINTENANCE_RUNTIME_DIR:-/run/cardputerzero-maintenance}
authorized_keys=$runtime_dir/authorized_keys
host_key=$runtime_dir/ssh_host_ed25519_key
config=${CP0_MAINTENANCE_SSHD_CONFIG:-/usr/lib/cardputerzero/maintenance-sshd_config}
ssh_keygen=${CP0_MAINTENANCE_SSH_KEYGEN:-/usr/bin/ssh-keygen}
sshd=${CP0_MAINTENANCE_SSHD:-/usr/sbin/sshd}
hostname_command=${CP0_MAINTENANCE_HOSTNAME:-/usr/bin/hostname}
runtime_owner=${CP0_MAINTENANCE_RUNTIME_OWNER:-root}
runtime_group=${CP0_MAINTENANCE_RUNTIME_GROUP:-root}

reject() {
    echo "cardputerzero-maintenance: $1" >&2
    exit 1
}

[ -f "$enable_file" ] && [ ! -L "$enable_file" ] ||
    reject "enable marker is not a regular file"
[ -f "$key_file" ] && [ ! -L "$key_file" ] ||
    reject "authorized key is not a regular file"
[ "$(wc -c <"$enable_file" | awk '{ print $1 }')" -le 64 ] ||
    reject "enable marker is too large"
[ "$(wc -c <"$key_file" | awk '{ print $1 }')" -le 1024 ] ||
    reject "authorized key is too large"
[ "$(cat "$enable_file")" = cp0-maintenance-v1 ] ||
    reject "enable marker version is invalid"

install -d -o "$runtime_owner" -g "$runtime_group" -m 0700 "$runtime_dir"
tr -d '\r' <"$key_file" >"$runtime_dir/authorized_keys.candidate"
[ "$(awk 'END { print NR }' "$runtime_dir/authorized_keys.candidate")" = 1 ] ||
    reject "exactly one public key is required"
awk '
    NF < 2 || $1 != "ssh-ed25519" || length($0) > 1024 { exit 1 }
' "$runtime_dir/authorized_keys.candidate" ||
    reject "only one bounded ED25519 public key is accepted"
"$ssh_keygen" -l -f "$runtime_dir/authorized_keys.candidate" >/dev/null ||
    reject "authorized key is invalid"
install -o "$runtime_owner" -g "$runtime_group" -m 0600 \
    "$runtime_dir/authorized_keys.candidate" "$authorized_keys"
rm -f "$runtime_dir/authorized_keys.candidate"

rm -f "$host_key" "$host_key.pub"
"$ssh_keygen" -q -t ed25519 -N '' -f "$host_key"
chmod 0600 "$host_key"
chmod 0644 "$host_key.pub"
"$sshd" -t -f "$config"

fingerprint=$("$ssh_keygen" -l -E sha256 -f "$host_key.pub" |
    awk '{ print $2 }')
addresses=$("$hostname_command" -I 2>/dev/null | awk '{$1=$1; print}' || true)
status_new=$status_file.new
[ ! -L "$status_file" ] && [ ! -L "$status_new" ] ||
    reject "status path must not be symbolic"
umask 077
{
    printf '%s\n' 'cp0-maintenance-status-v1'
    printf 'host-key %s\n' "$fingerprint"
    printf 'ipv4 %s\n' "${addresses:-pending}"
    printf '%s\n' 'login root'
} >"$status_new"
mv -f "$status_new" "$status_file"

# Possession of the writable boot media is the one-time authorization event.
# Consume both inputs before exposing the listener so a reboot fails closed.
rm -f "$enable_file" "$key_file"
sync
echo "cardputerzero-maintenance: one-boot SSH enabled; host-key=$fingerprint ipv4=${addresses:-pending}"
