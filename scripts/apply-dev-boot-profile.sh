#!/usr/bin/env bash
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
    echo "error: run as root on the CardputerZero device" >&2
    exit 1
fi

model=$(tr -d '\000' </proc/device-tree/model 2>/dev/null || true)
if [[ "$model" != *"Compute Module 0"* ]]; then
    echo "error: refusing to modify an unexpected device: ${model:-unknown}" >&2
    exit 1
fi

boot_dir=/boot/firmware
config="$boot_dir/config.txt"
cmdline="$boot_dir/cmdline.txt"
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
dtb_source="$script_dir/bcm2710-rpi-cm0.dtb"
dtb_target="$boot_dir/bcm2710-rpi-cm0.dtb"
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
backup_dir="$boot_dir/cardputerzero-os-backup/$timestamp"

test -f "$config"
test -f "$cmdline"
for camera_firmware in start_x.elf fixup_x.dat; do
    if [[ ! -s $boot_dir/$camera_firmware ]]; then
        echo "error: standard camera firmware is missing: $camera_firmware" >&2
        exit 1
    fi
done
legacy_bootscreen_sha256=d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd
if [[ $(sha256sum "$boot_dir/start_x.elf" | awk '{print $1}') == \
      "$legacy_bootscreen_sha256" ]]; then
    echo "error: refusing the M5Stack firmware that forces 256 MB GPU memory" >&2
    exit 1
fi
if [[ ! -f "$dtb_source" ]]; then
    echo "error: missing $dtb_source; run patch-cm0-dtb.sh and upload the DTB next to this script" >&2
    exit 1
fi
if strings "$dtb_source" | grep -q 'cgroup_disable=memory'; then
    echo "error: patched DTB still contains cgroup_disable=memory" >&2
    exit 1
fi
mkdir -p "$backup_dir"
cp -a "$config" "$cmdline" "$dtb_target" "$backup_dir/"
install -m 0644 "$dtb_source" "$dtb_target"

sed -i -E \
    's/^dtoverlay=vc4-kms-v3d(,cma-[0-9]+)?[[:space:]]*$/dtoverlay=vc4-kms-v3d,cma-64/' \
    "$config"
sed -i '/^dtoverlay=cardputerzero-os-bootargs-overlay$/d' "$config"
rm -f "$boot_dir/overlays/cardputerzero-os-bootargs-overlay.dtbo"
if ! grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$config"; then
    sed -i '/^\[all\]$/i dtoverlay=vc4-kms-v3d,cma-64' "$config"
fi

sed -i '/^# BEGIN CARDPUTERZERO OS DEV PROFILE$/,/^# END CARDPUTERZERO OS DEV PROFILE$/d' "$config"
cat >>"$config" <<'CONFIG'

# BEGIN CARDPUTERZERO OS DEV PROFILE
[all]
gpu_mem=64
gpu_mem_512=64
start_x=1
# END CARDPUTERZERO OS DEV PROFILE
CONFIG

cmdline_value=$(cat "$cmdline")
for token in cgroup_memory=1 cgroup_enable=memory apparmor=1 security=apparmor; do
    if [[ " $cmdline_value " != *" $token "* ]]; then
        cmdline_value+=" $token"
    fi
done
printf '%s\n' "$cmdline_value" >"$cmdline"

sync
echo "CardputerZero OS development boot profile installed."
echo "Backup: $backup_dir"
echo "A reboot is required. This script did not reboot the device."
