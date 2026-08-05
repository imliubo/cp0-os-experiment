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
camera_probe="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/camera-probe.sh"
camera_probe_unit="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-camera-probe.service"
backlight_patch="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/0002-cardputerzero-v06-backlight-zero-duty.patch"
rootfs_verifier="$repo_root/tests/test-built-rootfs-profile.sh"
makefile="$repo_root/Makefile"
boot_fragment="$repo_root/bsp/cm0-v0.6/boot/config.txt.fragment"
cmdline_fragment="$repo_root/bsp/cm0-v0.6/boot/cmdline.extra"
bootscreen_firmware="$repo_root/bsp/cm0-v0.6/firmware/start-m5stack-bootscreen.elf"
splash_png="$repo_root/bsp/cm0-v0.6/boot/splash.png"
splash_bmp="$repo_root/bsp/cm0-v0.6/boot/splash.bmp"

expected_firmware_sha256=d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd
expected_fixup_sha256=b2d19b8c300b5a4ddbd0fcff3a0f7de61a171046269d8724e74f616058417d4b
expected_splash_png_sha256=17b6b5571fd3be038992df24134d7ca88c75b22cb36e84cf2f007664096298e1
expected_splash_bmp_sha256=dfaf289bae036e60014093cdf2705ab50d33507c38d6d197640fda99e32efc30

grep -q '^PI_GEN_BRANCH=arm64$' "$repo_root/image/pi-gen/upstream.env"
grep -Eq '^PI_GEN_COMMIT=[0-9a-f]{40}$' "$repo_root/image/pi-gen/upstream.env"
grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$stage"
grep -q '^camera_auto_detect=0$' "$stage"
grep -q "'/^start_x=/d'" "$stage"
if grep -q '^start_x=' "$stage" || grep -q '^start_x=' "$boot_fragment"; then
    echo "error: M5Stack boot-screen firmware must use the official start.elf path" >&2
    exit 1
fi
grep -q '^dtoverlay=imx219$' "$stage"
if grep -q '^dtoverlay=camera-py12-high-overlay$' "$stage"; then
    echo "error: V0.6 must leave P12 under the powerfail driver" >&2
    exit 1
fi
grep -q "'/^camera_auto_detect=/d'" "$stage"
if grep -q '^dtoverlay=camera-gpio16-high-overlay$' "$stage"; then
    echo "error: V0.6 must not enable the legacy camera GPIO16 overlay" >&2
    exit 1
fi
for camera_config in "$stage" "$boot_fragment"; do
    card_line=$(grep -n '^dtoverlay=cardputerzero-v5-overlay$' "$camera_config" | tail -1 | cut -d: -f1)
    sensor_line=$(grep -n '^dtoverlay=imx219$' "$camera_config" | tail -1 | cut -d: -f1)
    if [[ -z $card_line || -z $sensor_line || $card_line -ge $sensor_line ]]; then
        echo "error: V0.6 camera overlays are not ordered board then sensor: $camera_config" >&2
        exit 1
    fi
    if grep -q '^dtoverlay=camera-py12-high-overlay$' "$camera_config"; then
        echo "error: V0.6 camera config conflicts with powerfail P12 ownership: $camera_config" >&2
        exit 1
    fi
    if grep -q '^dtoverlay=camera-gpio16-high-overlay$' "$camera_config"; then
        echo "error: V0.6 must not enable the legacy camera GPIO16 overlay: $camera_config" >&2
        exit 1
    fi
done
grep -q '^gpu_mem=64$' "$stage"
grep -q '^gpu_mem_512=64$' "$stage"
grep -Fq "BOOTSCREEN_FIRMWARE_SHA256=\"$expected_firmware_sha256\"" "$stage"
grep -Fq "BOOTSCREEN_FIXUP_SHA256=\"$expected_fixup_sha256\"" "$stage"
grep -Fq "BOOTSCREEN_SPLASH_SHA256=\"$expected_splash_bmp_sha256\"" "$stage"
grep -Fq 'boot_tokens='"'"'quiet loglevel=3 logo.nologo vt.global_cursor_default=0 consoleblank=0 fbcon=map:off systemd.show_status=false rd.systemd.show_status=false'"'" "$stage"
grep -Fq 'boot_tokens='"'"'loglevel=6 consoleblank=0 fbcon=map:1'"'" "$stage"
for token in quiet loglevel=3 logo.nologo vt.global_cursor_default=0 \
    consoleblank=0 fbcon=map:off systemd.show_status=false \
    rd.systemd.show_status=false; do
    grep -Fqw "$token" "$cmdline_fragment"
done
test "$(wc -c <"$bootscreen_firmware" | tr -d ' ')" = 3055976
test "$(wc -c <"$splash_bmp" | tr -d ' ')" = 108866
test "$(shasum -a 256 "$bootscreen_firmware" | awk '{print $1}')" = \
    "$expected_firmware_sha256"
test "$(shasum -a 256 "$splash_png" | awk '{print $1}')" = \
    "$expected_splash_png_sha256"
test "$(shasum -a 256 "$splash_bmp" | awk '{print $1}')" = \
    "$expected_splash_bmp_sha256"
grep -Fq 'start-m5stack-bootscreen.elf' "$build_script"
grep -Fq 'bsp/cm0-v0.6/boot/splash.bmp' "$build_script"
grep -Fq 'boot/firmware/start.elf' "$stage"
grep -Fq 'boot/firmware/fixup.dat' "$stage"
grep -Fq 'boot/firmware/splash.bmp' "$stage"
grep -q 'fb_load.service' "$stage"
grep -q 'rm -f /etc/systemd/system/fb_load.service' "$stage"
grep -q 'cardputerzero-console-banner.service' "$stage"
grep -q 'systemctl disable cardputerzero-console-banner.service' "$stage"
grep -q 'cardputerzero-camera-probe.service' "$stage"
grep -qx 'Before=cardputerzero-camerad.socket cardputerzero-camerad.service' "$camera_probe_unit"
grep -q 'powerfail-suo' "$camera_probe"
grep -q 'drivers/imx219' "$camera_probe"
grep -q '10-0010' "$camera_probe"
grep -q 'kernel-camera.log' "$camera_probe"
grep -q 'schema=cardputerzero-camera-probe-v2' "$camera_probe"
grep -Fq 'CP0_CAMERA_PROBE_FIRMWARE_FILE:-/boot/firmware/start.elf' "$camera_probe"
grep -q '^firmware_mode=start$' "$camera_probe"
grep -q "printf 'firmware_mode=%s" "$camera_probe"
grep -q "printf 'firmware_variant=%s" "$camera_probe"
grep -q "printf 'firmware_sha256=%s" "$camera_probe"
grep -Fq "$expected_firmware_sha256" "$camera_probe"
grep -q 'count >= 100' "$camera_probe"
grep -q 'imx219|m5ioe1|powerfail|unicam' "$camera_probe"
if grep -q 'cp0-developer-access' "$camera_probe"; then
    echo "error: early camera probe cannot depend on the provisioned owner group" >&2
    exit 1
fi
grep -q 'Camera absence must not prevent' "$camera_probe"
if grep -Eq '(^|[[:space:]])(sudo|su)([[:space:]]|$)' "$camera_probe"; then
    echo "error: camera probe must not delegate general privilege" >&2
    exit 1
fi
sh -n "$camera_probe"
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
grep -q '0002-cardputerzero-v06-backlight-zero-duty.patch' "$stage"
grep -Fq "power-supply = <&backlight_power>;" "$backlight_patch"
grep -Fq "V0.6 backlight must keep zero-duty PWM actively driven" "$stage"
grep -Fq "grep -aFq 'power-supply'" "$rootfs_verifier"
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
