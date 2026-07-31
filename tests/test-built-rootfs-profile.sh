#!/usr/bin/env bash
set -euo pipefail

rootfs=${1:-${ROOTFS_DIR:-}}
if [[ -z $rootfs || ! -d $rootfs ]]; then
    echo "usage: $0 ROOTFS_DIR" >&2
    exit 2
fi
rootfs=$(cd "$rootfs" && pwd -P)
bootfs="$rootfs/boot/firmware"

required_executables=(
    usr/bin/cardputerzero-system-shell
    usr/bin/cp0ctl
    usr/libexec/cardputerzero/app-runtime
    usr/libexec/cardputerzero/cp0-appd
    usr/libexec/cardputerzero/cp0-audiod
    usr/libexec/cardputerzero/cp0-camerad
    usr/libexec/cardputerzero/cp0-documentd
    usr/libexec/cardputerzero/cp0-gpiod
    usr/libexec/cardputerzero/cp0-networkd
    usr/libexec/cardputerzero/cp0-radiod
    usr/libexec/cardputerzero/cp0-storaged
    usr/libexec/cardputerzero/device-core-recovery
    usr/libexec/cardputerzero/device-smoke.sh
    usr/libexec/cardputerzero/device-stability-monitor
    usr/libexec/cardputerzero/overlay-root-initramfs
)
for path in "${required_executables[@]}"; do
    if [[ ! -x $rootfs/$path || -L $rootfs/$path ]]; then
        echo "error: required executable missing or symbolic: /$path" >&2
        exit 1
    fi
done

required_files=(
    usr/lib/aarch64-linux-gnu/weston/cardputerzero-policy.so
    var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm
)
for path in "${required_files[@]}"; do
    if [[ ! -f $rootfs/$path || -L $rootfs/$path ]]; then
        echo "error: required image file missing or symbolic: /$path" >&2
        exit 1
    fi
done

enabled_units=(
    multi-user.target.wants/cardputerzero-compositor.service
    multi-user.target.wants/cardputerzero-console-banner.service
    multi-user.target.wants/cardputerzero-overlay-root-status.service
    sockets.target.wants/cardputerzero-appd.socket
    sockets.target.wants/cardputerzero-audiod.socket
    sockets.target.wants/cardputerzero-broker.socket
    sockets.target.wants/cardputerzero-camerad.socket
    sockets.target.wants/cardputerzero-documentd.socket
    sockets.target.wants/cardputerzero-gpiod.socket
    sockets.target.wants/cardputerzero-networkd.socket
    sockets.target.wants/cardputerzero-radiod.socket
    sockets.target.wants/cardputerzero-storaged.socket
)
for path in "${enabled_units[@]}"; do
    if [[ ! -L $rootfs/etc/systemd/system/$path ]]; then
        echo "error: required unit is not enabled: $path" >&2
        exit 1
    fi
done

grep -Rqx 'Storage=volatile' \
    "$rootfs/etc/systemd/journald.conf" \
    "$rootfs/etc/systemd/journald.conf.d"
grep -qE '^tmpfs[[:space:]]+/tmp[[:space:]]+tmpfs' "$rootfs/etc/fstab"
grep -qE '^tmpfs[[:space:]]+/var/tmp[[:space:]]+tmpfs' "$rootfs/etc/fstab"
grep -qx 'kernel.core_pattern=/dev/null' \
    "$rootfs/etc/sysctl.d/90-cardputerzero-os.conf"
grep -qx 'kernel.unprivileged_bpf_disabled=1' \
    "$rootfs/etc/sysctl.d/90-cardputerzero-os.conf"

test -s "$bootfs/initramfs8"
grep -qx 'auto_initramfs=1' "$bootfs/config.txt"
if grep -qw 'cp0.overlay_root=volatile' "$bootfs/cmdline.txt"; then
    echo "error: volatile overlay root must remain opt-in" >&2
    exit 1
fi
initramfs_contents=$(chroot "$rootfs" \
    /usr/bin/lsinitramfs /boot/firmware/initramfs8)
grep -qx 'scripts/init-bottom/cardputerzero-overlay-root' \
    <<<"$initramfs_contents"
grep -qE 'usr/lib/modules/.*/kernel/fs/overlayfs/overlay\.ko' \
    <<<"$initramfs_contents"
grep -qx 'usr/lib/firmware/cardputerzero,st7789v_lcd.bin' \
    <<<"$initramfs_contents"

if [[ -e $rootfs/etc/apt/apt.conf.d/51cache ]]; then
    echo "error: build proxy configuration leaked into the image" >&2
    exit 1
fi
if find "$rootfs" -xdev -type f -perm /0002 -print -quit | grep -q .; then
    echo "error: image contains a world-writable regular file" >&2
    exit 1
fi

echo "PASS built rootfs and initramfs profile"
