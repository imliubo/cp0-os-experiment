#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
bsp="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
gate="$repo_root/appd/systemd/cardputerzero-ssh-gate.conf"
generator="$repo_root/appd/systemd/cardputerzero-ssh-generator"
sshd_config="$repo_root/appd/systemd/cardputerzero-sshd.conf"
avahi="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/avahi-daemon.conf"

for path in \
    scripts/enable-maintenance-ssh.sh \
    scripts/device-hot-update-firstboot.sh \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/prepare-maintenance-ssh.sh \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/hot-update-firstboot.sh \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-maintenance-ssh.service \
    image/pi-gen/stage-cardputerzero-os/00-bsp/files/maintenance-sshd_config; do
    if [[ -e $repo_root/$path || -L $repo_root/$path ]]; then
        echo "error: pre-Setup maintenance access remains: $path" >&2
        exit 1
    fi
done

if grep -Eq 'maintenance-ssh|cp0-maintenance|hot-update-firstboot' "$bsp"; then
    echo "error: production BSP still installs pre-Setup remote access" >&2
    exit 1
fi
if grep -q '^host-name=cardputerzero-maintenance$' "$avahi"; then
    echo "error: removed maintenance identity remains advertised" >&2
    exit 1
fi

test "$(grep -c '^ConditionPathExists=/var/lib/cardputerzero/provisioning/complete$' "$gate")" -eq 1
test "$(grep -c '^ConditionPathExists=/var/lib/cardputerzero/provisioning/ssh-enabled$' "$gate")" -eq 1
grep -Fq 'complete=/var/lib/cardputerzero/provisioning/complete' "$generator"
grep -Fq 'enabled=/var/lib/cardputerzero/provisioning/ssh-enabled' "$generator"
grep -Fq '[ -f "$complete" ] && [ ! -L "$complete" ] || exit 0' "$generator"
grep -Fq '[ -f "$enabled" ] && [ ! -L "$enabled" ] || exit 0' "$generator"
grep -qx 'PermitRootLogin no' "$sshd_config"
grep -qx 'AllowGroups cp0-ssh' "$sshd_config"
grep -qx 'PasswordAuthentication yes' "$sshd_config"

bash -n "$bsp" "$generator"
printf '%s\n' 'setup SSH boundary tests passed'
