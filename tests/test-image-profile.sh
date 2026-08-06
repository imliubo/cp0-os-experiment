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
early_splash="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/show-early-splash.sh"
early_splash_spi="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/early-splash-spi.c"
initramfs_splash="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/early-splash-initramfs"
early_splash_unit="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/cardputerzero-early-splash.service"
backlight_patch="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/0002-cardputerzero-v06-backlight-zero-duty.patch"
rootfs_verifier="$repo_root/tests/test-built-rootfs-profile.sh"
dev_boot_profile="$repo_root/scripts/apply-dev-boot-profile.sh"
makefile="$repo_root/Makefile"
boot_fragment="$repo_root/bsp/cm0-v0.6/boot/config.txt.fragment"
cmdline_fragment="$repo_root/bsp/cm0-v0.6/boot/cmdline.extra"
splash_png="$repo_root/bsp/cm0-v0.6/boot/splash.png"
splash_rgb565="$repo_root/bsp/cm0-v0.6/boot/splash.rgb565"

legacy_firmware_sha256=d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd
expected_splash_png_sha256=17b6b5571fd3be038992df24134d7ca88c75b22cb36e84cf2f007664096298e1
expected_splash_rgb565_sha256=75a53d81f5ec087536a030919698c595630d48296e07d5f5f3d04ebebf2efd57

grep -q '^PI_GEN_BRANCH=arm64$' "$repo_root/image/pi-gen/upstream.env"
grep -Eq '^PI_GEN_COMMIT=[0-9a-f]{40}$' "$repo_root/image/pi-gen/upstream.env"
grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$stage"
grep -q '^camera_auto_detect=0$' "$stage"
grep -q "'/^start_x=/d'" "$stage"
grep -q '^start_x=1$' "$stage"
grep -q '^start_x=1$' "$boot_fragment"
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
grep -Fq "LEGACY_BOOTSCREEN_FIRMWARE_SHA256=\"$legacy_firmware_sha256\"" "$stage"
grep -Fq "EARLY_SPLASH_SHA256=\"$expected_splash_rgb565_sha256\"" "$stage"
grep -Fq 'for camera_firmware in start_x.elf fixup_x.dat' "$stage"
grep -Fq 'boot_tokens='"'"'quiet loglevel=3 logo.nologo vt.global_cursor_default=0 consoleblank=0 fbcon=map:off systemd.show_status=false rd.systemd.show_status=false'"'" "$stage"
grep -Fq 'boot_tokens='"'"'loglevel=6 consoleblank=0 fbcon=map:1'"'" "$stage"
for token in quiet loglevel=3 logo.nologo vt.global_cursor_default=0 \
    consoleblank=0 fbcon=map:off systemd.show_status=false \
    rd.systemd.show_status=false; do
    grep -Fqw "$token" "$cmdline_fragment"
done
test "$(wc -c <"$splash_rgb565" | tr -d ' ')" = 108800
test "$(shasum -a 256 "$splash_png" | awk '{print $1}')" = \
    "$expected_splash_png_sha256"
test "$(shasum -a 256 "$splash_rgb565" | awk '{print $1}')" = \
    "$expected_splash_rgb565_sha256"
if grep -Fq 'start-m5stack-bootscreen.elf' "$build_script"; then
    echo "error: image builder still packages the legacy M5Stack firmware" >&2
    exit 1
fi
grep -Fq 'bsp/cm0-v0.6/boot/splash.rgb565' "$build_script"
grep -Fq 'usr/share/cardputerzero/boot/splash.rgb565' "$stage"
grep -q 'cardputerzero-early-splash.service' "$stage"
grep -qx 'Before=cardputerzero-compositor.service cardputerzero-display-retry.service' \
    "$early_splash_unit"
grep -qx 'RequiresMountsFor=/var/lib/cardputerzero/registry' "$early_splash_unit"
grep -qx 'WantedBy=multi-user.target' "$early_splash_unit"
test -x "$initramfs_splash"
sh -n "$initramfs_splash"
grep -Fq 'scripts/init-top/cardputerzero-early-splash' "$stage"
grep -Fq '/usr/share/cardputerzero/boot/splash.rgb565' "$firmware_hook"
grep -Fq 'copy_exec /usr/libexec/cardputerzero/early-splash-spi' "$firmware_hook"
grep -Fq '/usr/libexec/cardputerzero/show-early-splash.sh' "$firmware_hook"
grep -Fq 'image-profile 2>/dev/null || true)" = product' "$firmware_hook"
grep -Fq 'mknod -m 0600 /dev/mem c 1 1' "$initramfs_splash"
grep -Fq 'timeout -s KILL 2 "$spi_renderer" "$splash"' "$initramfs_splash"
grep -Fq '"$framebuffer_renderer" >/dev/null 2>&1 &' "$initramfs_splash"
spi_line=$(grep -n 'timeout -s KILL 2 "$spi_renderer" "$splash"' \
    "$initramfs_splash" | cut -d: -f1)
framebuffer_line=$(grep -n '"$framebuffer_renderer" >/dev/null 2>&1 &' \
    "$initramfs_splash" | cut -d: -f1)
if [[ -z $spi_line || -z $framebuffer_line || $spi_line -ge $framebuffer_line ]]; then
    echo "error: direct SPI splash must run before the framebuffer fallback" >&2
    exit 1
fi
grep -Fq 'early-splash-spi.c' "$stage"
grep -Fq 'gcc -std=c11 -static -Os' "$stage"
grep -Fq 'e05b81c80f1f5a8e589956937adba5b5d04f0ca9' "$early_splash_spi"
grep -Fq '#define BCM2837_PERIPHERAL_BASE 0x3f000000UL' "$early_splash_spi"
if grep -Fq 'PERIPHERAL_BASE 0x20000000UL' "$early_splash_spi"; then
    echo "error: direct SPI splash uses the BCM2835 base on BCM2837 V0.6" >&2
    exit 1
fi
grep -Fq '#define DISPLAY_Y_OFFSET 35U' "$early_splash_spi"
grep -Fq '*reg(spi_registers, SPI_CLK) = 12U;' "$early_splash_spi"
grep -Fq 'SPI_WAIT_LIMIT' "$early_splash_spi"
grep -Fq '#define RENDER_TIMEOUT_SECONDS 2U' "$early_splash_spi"
grep -Fq 'static int drain_receive_fifo(void)' "$early_splash_spi"
grep -Fq '(void)alarm(RENDER_TIMEOUT_SECONDS);' "$early_splash_spi"
cc -std=c11 -Wall -Wextra -Werror -fsyntax-only "$early_splash_spi"
grep -q 'panel-mipi-dbid' "$early_splash"
grep -q '320,170' "$early_splash"
grep -q 'expected_bytes=108800' "$early_splash"
sh -n "$early_splash"
mkdir -p "$repo_root/target/test-tmp"
early_splash_tmp=$(mktemp -d "$repo_root/target/test-tmp/early-splash.XXXXXX")
trap 'rm -rf -- "$early_splash_tmp"' EXIT
cc -std=c11 -O2 -Wall -Wextra -Werror "$early_splash_spi" \
    -o "$early_splash_tmp/early-splash-spi"
"$early_splash_tmp/early-splash-spi" --check-image "$splash_rgb565"
if "$early_splash_tmp/early-splash-spi" --check-image "$splash_png"; then
    echo "error: direct SPI splash accepts a resource with the wrong size" >&2
    exit 1
fi
mkdir -p "$early_splash_tmp/sys/class/graphics/fb7" "$early_splash_tmp/dev"
printf 'panel-mipi-dbid\n' >"$early_splash_tmp/sys/class/graphics/fb7/name"
printf '320,170\n' >"$early_splash_tmp/sys/class/graphics/fb7/virtual_size"
printf '16\n' >"$early_splash_tmp/sys/class/graphics/fb7/bits_per_pixel"
printf '4\n' >"$early_splash_tmp/sys/class/graphics/fb7/blank"
: >"$early_splash_tmp/dev/fb7"
CP0_EARLY_SPLASH_UID=0 \
CP0_EARLY_SPLASH_SYSFS_ROOT="$early_splash_tmp/sys" \
CP0_EARLY_SPLASH_DEVICE_ROOT="$early_splash_tmp/dev" \
CP0_EARLY_SPLASH_FILE="$splash_rgb565" \
    sh "$early_splash" >/dev/null
test "$(wc -c <"$early_splash_tmp/dev/fb7" | tr -d ' ')" = 108800
cmp "$splash_rgb565" "$early_splash_tmp/dev/fb7"
grep -qx '0' "$early_splash_tmp/sys/class/graphics/fb7/blank"
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
grep -Fq 'firmware_file=/boot/firmware/start_x.elf' "$camera_probe"
grep -q '^firmware_mode=start$' "$camera_probe"
grep -q "printf 'firmware_mode=%s" "$camera_probe"
grep -q "printf 'firmware_variant=%s" "$camera_probe"
grep -q "printf 'firmware_sha256=%s" "$camera_probe"
grep -Fq "$legacy_firmware_sha256" "$camera_probe"
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
grep -q '^start_x=1$' "$dev_boot_profile"
grep -q 'for camera_firmware in start_x.elf fixup_x.dat' "$dev_boot_profile"
grep -Fq "$legacy_firmware_sha256" "$dev_boot_profile"
sh -n "$dev_boot_profile"
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
grep -qx 'dosfstools' "$packages"
grep -qx 'dtoverlay=dwc2,dr_mode=peripheral' "$boot_fragment"
grep -Fq "'dtoverlay=dwc2,dr_mode=peripheral'" "$stage"
grep -Fq 'dtoverlay=dwc2,dr_mode=peripheral' "$stage"
grep -q 'tca8418_keypad_m5stack' "$stage"
grep -q 'usb-configfs' "$smoke_script"
grep -q 'usb-udc' "$smoke_script"
grep -q 'loop-control' "$smoke_script"
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
