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
    usr/libexec/cardputerzero/cp0-stored
    usr/libexec/cardputerzero/device-core-recovery
    usr/libexec/cardputerzero/device-factory-acceptance
    usr/libexec/cardputerzero/device-smoke.sh
    usr/libexec/cardputerzero/device-stability-monitor
    usr/libexec/cardputerzero/device-support-bundle
    usr/libexec/cardputerzero/data-grow-initramfs
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
    etc/cardputerzero/device-policy.json
    var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm
)
for path in "${required_files[@]}"; do
    if [[ ! -f $rootfs/$path || -L $rootfs/$path ]]; then
        echo "error: required image file missing or symbolic: /$path" >&2
        exit 1
    fi
done
for marker in developer-mode recovery-mode; do
    if [[ -e $rootfs/var/lib/cardputerzero/registry/$marker ]]; then
        echo "error: product image enables $marker by default" >&2
        exit 1
    fi
done

enabled_units=(
    multi-user.target.wants/cardputerzero-compositor.service
    multi-user.target.wants/cardputerzero-console-banner.service
    multi-user.target.wants/cardputerzero-overlay-root-status.service
    multi-user.target.wants/cardputerzero-recovery-console.service
    sockets.target.wants/cardputerzero-appd.socket
    sockets.target.wants/cardputerzero-audiod.socket
    sockets.target.wants/cardputerzero-broker.socket
    sockets.target.wants/cardputerzero-camerad.socket
    sockets.target.wants/cardputerzero-documentd.socket
    sockets.target.wants/cardputerzero-gpiod.socket
    sockets.target.wants/cardputerzero-networkd.socket
    sockets.target.wants/cardputerzero-radiod.socket
    sockets.target.wants/cardputerzero-storaged.socket
    sockets.target.wants/cardputerzero-stored.socket
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
if ! grep -qw 'cp0.overlay_root=volatile' "$bootfs/cmdline.txt"; then
    echo "error: immutable root is not enabled in the final image" >&2
    exit 1
fi
if grep -qw 'resize' "$bootfs/cmdline.txt"; then
    echo "error: upstream root resize would overwrite cp0-data" >&2
    exit 1
fi
initramfs_contents=$(chroot "$rootfs" \
    /usr/bin/lsinitramfs /boot/firmware/initramfs8)
grep -qx 'scripts/local-premount/cardputerzero-data-grow' \
    <<<"$initramfs_contents"
grep -qx 'scripts/init-bottom/cardputerzero-overlay-root' \
    <<<"$initramfs_contents"
grep -qE 'usr/lib/modules/.*/kernel/fs/overlayfs/overlay\.ko' \
    <<<"$initramfs_contents"
grep -qx 'usr/lib/firmware/cardputerzero,st7789v_lcd.bin' \
    <<<"$initramfs_contents"

data_root="$rootfs/var/lib/cardputerzero-persist"
if [[ $(findmnt -n -o FSTYPE --target "$data_root") != ext4 ]]; then
    echo "error: cp0-data is not mounted during image verification" >&2
    exit 1
fi
data_device=$(findmnt -n -o SOURCE --target "$data_root")
if [[ $(blkid -s LABEL -o value "$data_device") != cp0-data ]]; then
    echo "error: persistent filesystem label is not cp0-data" >&2
    exit 1
fi
grep -qx 'cp0-data-layout-v1' "$data_root/layout-version"
for path in cardputerzero etc-cardputerzero network-connections \
    network-state ssh; do
    if [[ ! -d $data_root/$path || -L $data_root/$path ]]; then
        echo "error: persistent layout directory is invalid: $path" >&2
        exit 1
    fi
done
for path in machine-id random-seed; do
    if [[ ! -f $data_root/$path || -L $data_root/$path ]]; then
        echo "error: persistent layout file is invalid: $path" >&2
        exit 1
    fi
done

if [[ -e $rootfs/etc/apt/apt.conf.d/51cache ]]; then
    echo "error: build proxy configuration leaked into the image" >&2
    exit 1
fi
if grep -R -E \
    '^[[:space:]]*(deb[[:space:]]+|URIs:[[:space:]]*)http://(deb\.debian\.org|archive\.raspberrypi\.com)' \
    "$rootfs/etc/apt/sources.list" "$rootfs/etc/apt/sources.list.d" \
    2>/dev/null; then
    echo "error: unencrypted Debian or Raspberry Pi apt source in image" >&2
    exit 1
fi
if find "$rootfs" -xdev -type f -perm /0002 -print -quit | grep -q .; then
    echo "error: image contains a world-writable regular file" >&2
    exit 1
fi
if find "$data_root" -xdev -type f -perm /0002 -print -quit | grep -q .; then
    echo "error: cp0-data contains a world-writable regular file" >&2
    exit 1
fi

echo "PASS built rootfs and initramfs profile"
