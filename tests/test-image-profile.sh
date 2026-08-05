#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
packages="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/00-packages-nr"
build_script="$repo_root/image/build-image.sh"
smoke_script="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/device-smoke.sh"
firmware_hook="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-firmware.initramfs-hook"
ssh_prepare="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/prepare-ssh.sh"
ssh_prepare_unit="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-ssh-prepare.service"
rootfs_verifier="$repo_root/tests/test-built-rootfs-profile.sh"
makefile="$repo_root/Makefile"
boot_fragment="$repo_root/bsp/cm0-v0.6/boot/config.txt.fragment"

grep -q '^PI_GEN_BRANCH=arm64$' "$repo_root/image/pi-gen/upstream.env"
grep -Eq '^PI_GEN_COMMIT=[0-9a-f]{40}$' "$repo_root/image/pi-gen/upstream.env"
grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$stage"
grep -q '^camera_auto_detect=0$' "$stage"
grep -q '^dtoverlay=imx219$' "$stage"
if [[ $(grep -c '^dtoverlay=camera-py12-high-overlay$' "$stage") -ne 1 ]]; then
    echo "error: V0.6 must enable the P12 camera power overlay exactly once" >&2
    exit 1
fi
grep -q "'/^camera_auto_detect=/d'" "$stage"
if grep -q '^dtoverlay=camera-gpio16-high-overlay$' "$stage"; then
    echo "error: V0.6 must not enable the legacy camera GPIO16 overlay" >&2
    exit 1
fi
for camera_config in "$stage" "$boot_fragment"; do
    card_line=$(grep -n '^dtoverlay=cardputerzero-v5-overlay$' "$camera_config" | tail -1 | cut -d: -f1)
    power_line=$(grep -n '^dtoverlay=camera-py12-high-overlay$' "$camera_config" | tail -1 | cut -d: -f1)
    sensor_line=$(grep -n '^dtoverlay=imx219$' "$camera_config" | tail -1 | cut -d: -f1)
    if [[ -z $card_line || -z $power_line || -z $sensor_line ||
          $card_line -ge $power_line || $power_line -ge $sensor_line ]]; then
        echo "error: V0.6 camera overlays are not ordered board, P12 power, sensor: $camera_config" >&2
        exit 1
    fi
    if grep -q '^dtoverlay=camera-gpio16-high-overlay$' "$camera_config"; then
        echo "error: V0.6 must not enable the legacy camera GPIO16 overlay: $camera_config" >&2
        exit 1
    fi
done
grep -q '^gpu_mem=64$' "$stage"
grep -q '^gpu_mem_512=64$' "$stage"
grep -q 'consoleblank=0' "$stage"
grep -q 'fbcon=map:1' "$stage"
grep -q 'for token in quiet splash fbcon=map:off fbcon=map:0' "$stage"
grep -q 'fb_load.service' "$stage"
grep -q 'rm -f /etc/systemd/system/fb_load.service' "$stage"
grep -q 'cardputerzero-console-banner.service' "$stage"
if grep -q 'getty@tty1.service cardputerzero-console-banner.service' "$stage"; then
    echo "error: tty1 cannot be statically enabled with the compositor" >&2
    exit 1
fi
grep -q 'cardputerzero-ssh-prepare.service' "$stage"
grep -q '^RequiredBy=ssh.service$' "$ssh_prepare_unit"
grep -q '^Before=ssh.service$' "$ssh_prepare_unit"
if grep -q 'ConditionFirstBoot' "$ssh_prepare_unit"; then
    echo "error: SSH preparation cannot depend on systemd first-boot detection" >&2
    exit 1
fi
grep -q '^/usr/bin/ssh-keygen -A$' "$ssh_prepare"
grep -q '^/usr/sbin/sshd -t$' "$ssh_prepare"
sh -n "$ssh_prepare"
grep -q 'rpi-resize.service 2>/dev/null' "$stage"
grep -qx 'raspberrypi-sys-mods' "$packages"
grep -qx 'firmware-brcm80211' "$packages"
grep -q 'tca8418_keypad_m5stack' "$stage"
grep -q "grep -Fc 'spi-max-frequency = <60000000>;'" "$stage"
grep -q "spi-max-frequency = <20000000>;" "$stage"
grep -q 'panel-mipi-dbid' "$smoke_script"
grep -q '/sys/bus/i2c/devices/i2c-1' "$smoke_script"
grep -q 'raw access disabled' "$smoke_script"
grep -q 'copy_file firmware /lib/firmware/cardputerzero,st7789v_lcd.bin' "$firmware_hook"
grep -q 'cardputerzero-firmware.initramfs-hook' "$stage"
grep -q 'export-image/05-finalise/01-run.sh' "$build_script"
grep -q 'cardputerzero-verify-rootfs.sh' "$build_script"
grep -q "pi-gen finalise unmount marker changed" "$build_script"
grep -Fq 'rm -rf "$pi_gen_dir/export-image/06-cardputerzero-verify"' \
    "$build_script"
if grep -q 'verify_stage=' "$build_script"; then
    echo "error: rootfs verification cannot use a post-finalise export stage" >&2
    exit 1
fi
grep -q 'export-image-prerun.sh' "$build_script"
grep -q 'scripts/init-bottom/cardputerzero-overlay-root' "$rootfs_verifier"
grep -Fq 'chroot "$rootfs" /usr/bin/unmkinitramfs' "$rootfs_verifier"
if grep -q '^unmkinitramfs ' "$rootfs_verifier"; then
    echo "error: rootfs verification depends on the pi-gen host tools" >&2
    exit 1
fi
grep -q 'build proxy configuration leaked' "$rootfs_verifier"
grep -q 'unencrypted Debian or Raspberry Pi apt source' "$rootfs_verifier"
grep -q 'https://archive.raspberrypi.com' "$stage"
sh -n "$firmware_hook"
grep -q 'systemctl set-default multi-user.target' "$stage"
grep -q '/pi-gen/stage0 /pi-gen/stage1 /pi-gen/stage-cardputerzero-os' "$build_script"
grep -q 'linux-image-rpi-2712 linux-headers-rpi-2712' "$build_script"
grep -q "Pi 5 kernel package remains in stage0" "$build_script"
grep -q "https://deb.debian.org" "$build_script"
grep -q "https://archive.raspberrypi.com" "$build_script"
grep -q 'Acquire::https' "$build_script"
grep -q 'export-image/01-user-rename/SKIP' "$build_script"
grep -q 'CP0_RESUME_BUILD' "$build_script"
grep -q -- '--volumes-from' "$build_script"
grep -Fq 'info_file="$${IMAGE_INFO:-}"' "$makefile"
grep -Fq 'ls -1t deploy/*.info' "$makefile"
grep -Fq './tests/test-built-image-profile.sh "$$info_file"' "$makefile"
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
