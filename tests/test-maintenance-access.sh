#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
prepare=$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/prepare-maintenance-ssh.sh
unit=$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-maintenance-ssh.service
config=$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/maintenance-sshd_config
hot_update=$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/hot-update-firstboot.sh
work=$repo_root/target/test-tmp/maintenance-access.$$
boot=$work/boot
runtime=$work/run
trap 'rm -rf "$work"' EXIT HUP INT TERM
mkdir -p "$boot" "$runtime"
touch "$boot/config.txt" "$boot/cmdline.txt"

ssh-keygen -q -t ed25519 -N '' -f "$work/operator"
helper_boot=$work/helper-boot
mkdir "$helper_boot"
touch "$helper_boot/config.txt" "$helper_boot/cmdline.txt"
"$repo_root/scripts/enable-maintenance-ssh.sh" \
    "$helper_boot" "$work/operator.pub" >/dev/null
grep -qx cp0-maintenance-v1 "$helper_boot/cp0-maintenance.enable"
cmp "$work/operator.pub" "$helper_boot/cp0-maintenance.authorized_key"

cp "$work/operator.pub" "$boot/cp0-maintenance.authorized_key"
printf '%s\n' cp0-maintenance-v1 >"$boot/cp0-maintenance.enable"
CP0_MAINTENANCE_BOOT_DIR=$boot \
CP0_MAINTENANCE_RUNTIME_DIR=$runtime \
CP0_MAINTENANCE_SSHD=/usr/bin/true \
CP0_MAINTENANCE_HOSTNAME=/usr/bin/false \
CP0_MAINTENANCE_NETWORK_WAIT_SECONDS=0 \
CP0_MAINTENANCE_SSHD_CONFIG=/dev/null \
CP0_MAINTENANCE_RUNTIME_OWNER=$(id -un) \
CP0_MAINTENANCE_RUNTIME_GROUP=$(id -gn) \
    "$prepare"

test ! -e "$boot/cp0-maintenance.enable"
test ! -e "$boot/cp0-maintenance.authorized_key"
grep -qx cp0-maintenance-status-v1 "$boot/cp0-maintenance.status"
grep -q '^host-key SHA256:' "$boot/cp0-maintenance.status"
grep -qx 'login root' "$boot/cp0-maintenance.status"
cmp "$work/operator.pub" "$runtime/authorized_keys"
test "$(stat -f '%Lp' "$runtime/authorized_keys" 2>/dev/null || stat -c '%a' "$runtime/authorized_keys")" = 600

if CP0_MAINTENANCE_BOOT_DIR=$boot \
   CP0_MAINTENANCE_RUNTIME_DIR=$runtime \
   CP0_MAINTENANCE_SSHD=/usr/bin/true \
   CP0_MAINTENANCE_HOSTNAME=/usr/bin/false \
   CP0_MAINTENANCE_NETWORK_WAIT_SECONDS=0 \
   CP0_MAINTENANCE_SSHD_CONFIG=/dev/null \
   CP0_MAINTENANCE_RUNTIME_OWNER=$(id -un) \
   CP0_MAINTENANCE_RUNTIME_GROUP=$(id -gn) \
       "$prepare" >/dev/null 2>&1; then
    echo "error: consumed maintenance request was accepted twice" >&2
    exit 1
fi

invalid_boot=$work/invalid-boot
invalid_runtime=$work/invalid-run
mkdir "$invalid_boot" "$invalid_runtime"
ssh-keygen -q -t rsa -b 2048 -N '' -f "$work/rsa"
cp "$work/rsa.pub" "$invalid_boot/cp0-maintenance.authorized_key"
printf '%s\n' cp0-maintenance-v1 >"$invalid_boot/cp0-maintenance.enable"
if CP0_MAINTENANCE_BOOT_DIR=$invalid_boot \
   CP0_MAINTENANCE_RUNTIME_DIR=$invalid_runtime \
   CP0_MAINTENANCE_SSHD=/usr/bin/true \
   CP0_MAINTENANCE_HOSTNAME=/usr/bin/false \
   CP0_MAINTENANCE_SSHD_CONFIG=/dev/null \
   CP0_MAINTENANCE_RUNTIME_OWNER=$(id -un) \
   CP0_MAINTENANCE_RUNTIME_GROUP=$(id -gn) \
       "$prepare" >/dev/null 2>&1; then
    echo "error: non-ED25519 maintenance key was accepted" >&2
    exit 1
fi
test -e "$invalid_boot/cp0-maintenance.enable"
test -e "$invalid_boot/cp0-maintenance.authorized_key"
test ! -e "$invalid_boot/cp0-maintenance.status"

grep -q '^AuthenticationMethods publickey$' "$config"
grep -q '^PasswordAuthentication no$' "$config"
grep -q '^PermitRootLogin prohibit-password$' "$config"
grep -q '^AllowUsers root$' "$config"
grep -q '^ConditionPathExists=/boot/firmware/cp0-maintenance.enable$' "$unit"
grep -q '^Conflicts=ssh.service$' "$unit"
grep -q '^host-name=cardputerzero-maintenance$' \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/avahi-daemon.conf"
grep -q '^avahi-daemon$' \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/00-packages-nr"
grep -q 'hot-update-firstboot.sh' "$repo_root/scripts/device-hot-update-firstboot.sh"
grep -q 'activation failed; restoring previous binaries' "$hot_update"
grep -q "stat -c '%u:%g:%a'" "$hot_update"
if grep -q '\[ -x "\$artifact" \]' "$hot_update"; then
    echo "error: noexec runtime artifacts are still checked with test -x" >&2
    exit 1
fi

for script in "$prepare" "$hot_update" \
    "$repo_root/scripts/enable-maintenance-ssh.sh" \
    "$repo_root/scripts/device-hot-update-firstboot.sh"; do
    sh -n "$script"
done
