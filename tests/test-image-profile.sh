#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
packages="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/00-packages-nr"
build_script="$repo_root/image/build-image.sh"
smoke_script="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/device-smoke.sh"
firmware_hook="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-firmware.initramfs-hook"
rootfs_verifier="$repo_root/tests/test-built-rootfs-profile.sh"

grep -q '^PI_GEN_BRANCH=arm64$' "$repo_root/image/pi-gen/upstream.env"
grep -Eq '^PI_GEN_COMMIT=[0-9a-f]{40}$' "$repo_root/image/pi-gen/upstream.env"
grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$stage"
grep -q '^gpu_mem=64$' "$stage"
grep -q '^gpu_mem_512=64$' "$stage"
grep -q 'consoleblank=0' "$stage"
grep -q 'fbcon=map:1' "$stage"
grep -q 'for token in quiet splash fbcon=map:off fbcon=map:0' "$stage"
grep -q 'fb_load.service' "$stage"
grep -q 'rm -f /etc/systemd/system/fb_load.service' "$stage"
grep -q 'getty@tty1.service cardputerzero-console-banner.service' "$stage"
grep -q 'rpi-resize.service 2>/dev/null' "$stage"
grep -qx 'raspberrypi-sys-mods' "$packages"
grep -q 'tca8418_keypad_m5stack' "$stage"
grep -q 'panel-mipi-dbid' "$smoke_script"
grep -q 'copy_file firmware /lib/firmware/cardputerzero,st7789v_lcd.bin' "$firmware_hook"
grep -q 'cardputerzero-firmware.initramfs-hook' "$stage"
grep -q '06-cardputerzero-verify' "$build_script"
grep -q 'export-image-prerun.sh' "$build_script"
grep -q 'scripts/init-bottom/cardputerzero-overlay-root' "$rootfs_verifier"
grep -q 'build proxy configuration leaked' "$rootfs_verifier"
sh -n "$firmware_hook"
grep -q 'systemctl set-default multi-user.target' "$stage"
grep -q '/pi-gen/stage0 /pi-gen/stage1 /pi-gen/stage-cardputerzero-os' "$build_script"
grep -q 'linux-image-rpi-2712 linux-headers-rpi-2712' "$build_script"
grep -q "Pi 5 kernel package remains in stage0" "$build_script"
grep -q "https://deb.debian.org" "$build_script"
grep -q 'Acquire::https' "$build_script"
grep -q 'export-image/01-user-rename/SKIP' "$build_script"
grep -q 'CP0_RESUME_BUILD' "$build_script"
grep -q -- '--volumes-from' "$build_script"
if grep -q '/pi-gen/stage2' "$build_script"; then
    echo "error: stage2 must not be part of the minimal image" >&2
    exit 1
fi
for package in lightdm wayfire wf-panel-pi pcmanfm packagekit pipewire; do
    if grep -qx "$package" "$packages"; then
        echo "error: prohibited GUI package in minimal image: $package" >&2
        exit 1
    fi
    grep -qw "$package" "$stage"
done
