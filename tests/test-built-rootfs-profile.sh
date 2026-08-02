#!/usr/bin/env bash
set -euo pipefail

rootfs=${1:-${ROOTFS_DIR:-}}
if [[ -z $rootfs || ! -d $rootfs ]]; then
    echo "usage: $0 ROOTFS_DIR" >&2
    exit 2
fi
rootfs=$(cd "$rootfs" && pwd -P)
bootfs="$rootfs/boot/firmware"
image_profile=$(cat "$rootfs/etc/cardputerzero/image-profile" 2>/dev/null || true)
case "$image_profile" in
    product | recovery) ;;
    *)
        echo "error: missing or invalid image profile: ${image_profile:-missing}" >&2
        exit 1
        ;;
esac
access_profile=$(cat "$rootfs/etc/cardputerzero/access-profile" 2>/dev/null || true)
case "$access_profile" in
    development | production) ;;
    *)
        echo "error: missing or invalid access profile: ${access_profile:-missing}" >&2
        exit 1
        ;;
esac
if [[ $image_profile == recovery && $access_profile != development ]]; then
    echo "error: recovery image has production access profile" >&2
    exit 1
fi

required_executables=(
    usr/bin/cardputerzero-system-shell
    usr/bin/cp0-recovery
    usr/bin/cp0ctl
    usr/libexec/cardputerzero/app-runtime
    usr/libexec/cardputerzero/cp0-appd
    usr/libexec/cardputerzero/cp0-audiod
    usr/libexec/cardputerzero/cp0-camerad
    usr/libexec/cardputerzero/cp0-documentd
    usr/libexec/cardputerzero/cp0-gpiod
    usr/libexec/cardputerzero/cp0-networkd
    usr/libexec/cardputerzero/cp0-provisiond
    usr/libexec/cardputerzero/cp0-radiod
    usr/libexec/cardputerzero/cp0-storaged
    usr/libexec/cardputerzero/cp0-stored
    usr/libexec/cardputerzero/device-core-recovery
    usr/libexec/cardputerzero/device-capability-acceptance
    usr/libexec/cardputerzero/device-factory-acceptance
    usr/libexec/cardputerzero/device-performance-acceptance
    usr/libexec/cardputerzero/device-recovery-data
    usr/libexec/cardputerzero/device-smoke.sh
    usr/libexec/cardputerzero/device-stability-monitor
    usr/libexec/cardputerzero/device-store-acceptance
    usr/libexec/cardputerzero/device-support-bundle
    usr/libexec/cardputerzero/data-grow-initramfs
    usr/libexec/cardputerzero/overlay-root-initramfs
    usr/libexec/cardputerzero/prepare-ssh.sh
    usr/lib/systemd/system-generators/cardputerzero-display-generator
    etc/initramfs-tools/scripts/init-bottom/cardputerzero-overlay-root
    etc/initramfs-tools/scripts/local-premount/cardputerzero-data-grow
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
        echo "error: image enables $marker by default" >&2
        exit 1
    fi
done

enabled_units=(
    multi-user.target.wants/cardputerzero-console-banner.service
)
if [[ $access_profile == development ]]; then
    enabled_units+=(
        multi-user.target.wants/ssh.service
        ssh.service.requires/cardputerzero-ssh-prepare.service
        sysinit.target.wants/regenerate_ssh_host_keys.service
    )
fi
if [[ $image_profile == product ]]; then
    enabled_units+=(
        multi-user.target.wants/cardputerzero-overlay-root-status.service
        multi-user.target.wants/seatd.service
        sockets.target.wants/cardputerzero-appd.socket
        sockets.target.wants/cardputerzero-audiod.socket
        sockets.target.wants/cardputerzero-broker.socket
        sockets.target.wants/cardputerzero-camerad.socket
        sockets.target.wants/cardputerzero-documentd.socket
        sockets.target.wants/cardputerzero-gpiod.socket
        sockets.target.wants/cardputerzero-networkd.socket
        sockets.target.wants/cardputerzero-provisiond.socket
        multi-user.target.wants/cardputerzero-provision-apply.service
        sockets.target.wants/cardputerzero-radiod.socket
        sockets.target.wants/cardputerzero-storaged.socket
        sockets.target.wants/cardputerzero-stored.socket
    )
fi
for path in "${enabled_units[@]}"; do
    if [[ ! -L $rootfs/etc/systemd/system/$path ]]; then
        echo "error: required unit is not enabled: $path" >&2
        exit 1
    fi
done
if [[ $access_profile == production ]]; then
    for unit in regenerate_ssh_host_keys.service \
        getty@.service getty@tty1.service serial-getty@.service \
        serial-getty@serial0.service cardputerzero-recovery-console.service; do
        mask="$rootfs/etc/systemd/system/$unit"
        if [[ ! -L $mask || $(readlink "$mask") != /dev/null ]]; then
            echo "error: production access unit is not masked: $unit" >&2
            exit 1
        fi
    done
    if awk -F: '$3 >= 1000 && $3 < 20000 { found=1 } END { exit !found }' \
        "$rootfs/etc/passwd"; then
        echo "error: production image contains a human account" >&2
        exit 1
    fi
    if grep -q '^cp0-build:' "$rootfs/etc/passwd" "$rootfs/etc/shadow" \
        "$rootfs/etc/group" || find "$rootfs" -xdev -uid 1000 -print -quit | grep -q .; then
        echo "error: production build identity residue remains" >&2
        exit 1
    fi
    if [[ -e $rootfs/etc/systemd/system/multi-user.target.wants/ssh.service ]]; then
        echo "error: production image enables SSH before owner consent" >&2
        exit 1
    fi
    test -x "$rootfs/usr/lib/systemd/system-generators/cardputerzero-ssh-generator"
    grep -qx 'AllowGroups cp0-ssh' \
        "$rootfs/etc/ssh/sshd_config.d/40-cardputerzero-owner.conf"
    grep -qx 'ProtectHome=no' \
        "$rootfs/usr/lib/systemd/system/cardputerzero-provisiond.service"
    for database in passwd group shadow; do
        grep -Eq "^${database}:.*(^|[[:space:]])extrausers([[:space:]]|$)" \
            "$rootfs/etc/nsswitch.conf"
    done
    test -e "$rootfs/usr/lib/libnss_extrausers.so.2"
    chroot "$rootfs" /usr/bin/dpkg-query -W -f='${Status}\n' \
        libnss-extrausers | grep -qx 'install ok installed'
    chroot "$rootfs" /usr/bin/locale -a | grep -qx 'en_US.utf8'
    chroot "$rootfs" /usr/bin/locale -a | grep -qx 'zh_CN.utf8'
    for database in passwd shadow group gshadow; do
        if [[ -s $rootfs/var/lib/cardputerzero-persist/extrausers/$database ]]; then
            echo "error: production image seeds an owner identity: $database" >&2
            exit 1
        fi
    done
    if [[ ! -d $rootfs/var/lib/cardputerzero-persist/cardputerzero/provisioning ]]; then
        echo "error: production image omits the persistent provisioning directory" >&2
        exit 1
    fi
    if [[ $(stat -c '%a:%u:%g' \
        "$rootfs/var/lib/cardputerzero-persist/cardputerzero/provisioning") != 700:0:0 ]]; then
        echo "error: persistent provisioning directory ownership or mode is unsafe" >&2
        exit 1
    fi
    chroot "$rootfs" /usr/bin/jq -e \
        '.developer_mode_allowed == false and .recovery_mode_allowed == false' \
        /etc/cardputerzero/device-policy.json >/dev/null
elif [[ ! -L $rootfs/etc/systemd/system/multi-user.target.wants/ssh.service ]]; then
    echo "error: development access does not enable SSH" >&2
    exit 1
fi
machine_id_commit_mask="$rootfs/etc/systemd/system/systemd-machine-id-commit.service"
if [[ $image_profile == product ]]; then
    if [[ ! -L $machine_id_commit_mask ||
          $(readlink "$machine_id_commit_mask") != /dev/null ]]; then
        echo "error: product image does not mask redundant machine-id commit" >&2
        exit 1
    fi
elif [[ -L $machine_id_commit_mask &&
        $(readlink "$machine_id_commit_mask") == /dev/null ]]; then
    echo "error: recovery image masks machine-id commit" >&2
    exit 1
fi
for path in getty.target.wants/getty@tty1.service \
    multi-user.target.wants/cardputerzero-compositor.service \
    multi-user.target.wants/cardputerzero-recovery-console.service; do
    if [[ -e $rootfs/etc/systemd/system/$path ||
          -L $rootfs/etc/systemd/system/$path ]]; then
        echo "error: display session is statically enabled: $path" >&2
        exit 1
    fi
done
if [[ $image_profile == recovery ]]; then
    masked_units=(
        cardputerzero-compositor.service
        cardputerzero-system-shell.service
        cardputerzero-appd.service
        cardputerzero-appd.socket
        cardputerzero-audiod.socket
        cardputerzero-broker.socket
        cardputerzero-camerad.socket
        cardputerzero-documentd.socket
        cardputerzero-gpiod.socket
        cardputerzero-networkd.socket
        cardputerzero-radiod.socket
        cardputerzero-storaged.socket
        cardputerzero-stored.socket
    )
    for unit in "${masked_units[@]}"; do
        mask="$rootfs/etc/systemd/system/$unit"
        if [[ ! -L $mask || $(readlink "$mask") != /dev/null ]]; then
            echo "error: recovery image unit is not masked: $unit" >&2
            exit 1
        fi
    done
fi

grep -Rqx 'Storage=volatile' \
    "$rootfs/etc/systemd/journald.conf" \
    "$rootfs/etc/systemd/journald.conf.d"
grep -qE '^tmpfs[[:space:]]+/tmp[[:space:]]+tmpfs' "$rootfs/etc/fstab"
grep -qE '^tmpfs[[:space:]]+/var/tmp[[:space:]]+tmpfs.*size=128M' \
    "$rootfs/etc/fstab"
grep -qx 'kernel.core_pattern=/dev/null' \
    "$rootfs/etc/sysctl.d/90-cardputerzero-os.conf"
grep -qx 'kernel.unprivileged_bpf_disabled=1' \
    "$rootfs/etc/sysctl.d/90-cardputerzero-os.conf"

test -s "$bootfs/initramfs8"
grep -qx 'auto_initramfs=1' "$bootfs/config.txt"
if [[ $image_profile == product ]]; then
    factory_bundle=/usr/share/cardputerzero/factory-data-v1.cp0backup
    test -s "$rootfs$factory_bundle"
    factory_summary=$(chroot "$rootfs" /usr/bin/cp0-recovery verify "$factory_bundle")
    if [[ $factory_summary != *" profile=product" ]]; then
        echo "error: product factory seed has the wrong profile" >&2
        exit 1
    fi
    if ! grep -qw 'cp0.overlay_root=volatile' "$bootfs/cmdline.txt"; then
        echo "error: immutable root is not enabled in the product image" >&2
        exit 1
    fi
elif grep -qw 'cp0.overlay_root=volatile' "$bootfs/cmdline.txt"; then
    echo "error: recovery image unexpectedly enables immutable root" >&2
    exit 1
fi
if [[ $image_profile == recovery &&
      -e $rootfs/usr/share/cardputerzero/factory-data-v1.cp0backup ]]; then
    echo "error: recovery image contains an incomplete product factory seed" >&2
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

verify_tmp_parent="$rootfs/run"
initramfs_extract=$(mktemp -d \
    "$verify_tmp_parent/cardputerzero-initramfs-order.XXXXXX")
case "$initramfs_extract" in
    "$verify_tmp_parent"/cardputerzero-initramfs-order.*) ;;
    *)
        echo "error: unsafe initramfs verification directory" >&2
        exit 1
        ;;
esac
trap 'rm -rf -- "$initramfs_extract"' EXIT
initramfs_chroot=${initramfs_extract#"$rootfs"}
case "$initramfs_chroot" in
    /run/cardputerzero-initramfs-order.*) ;;
    *)
        echo "error: initramfs verification directory is outside rootfs" >&2
        exit 1
        ;;
esac
chroot "$rootfs" /usr/bin/unmkinitramfs \
    /boot/firmware/initramfs8 "$initramfs_chroot"
grep -Fqx '/scripts/init-bottom/cardputerzero-overlay-root "$@"' \
    "$initramfs_extract/scripts/init-bottom/ORDER"
grep -Fqx '/scripts/local-premount/cardputerzero-data-grow "$@"' \
    "$initramfs_extract/scripts/local-premount/ORDER"

generator_output="$initramfs_extract/display-generator"
generator_chroot="$initramfs_chroot/display-generator"
mkdir -p "$generator_output/early" "$generator_output/late"
chroot "$rootfs" \
    /usr/lib/systemd/system-generators/cardputerzero-display-generator \
    "$generator_chroot" "$generator_chroot/early" "$generator_chroot/late"
if [[ $image_profile == product ]]; then
    selected_display=cardputerzero-compositor.service
else
    selected_display=cardputerzero-recovery-console.service
fi
test -L "$generator_output/multi-user.target.wants/$selected_display"
if [[ $(find "$generator_output/multi-user.target.wants" -type l | wc -l) -ne 1 ]]; then
    echo "error: display generator did not select exactly one session" >&2
    exit 1
fi

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
grep -qx 'cp0-data-layout-v2' "$data_root/layout-version"
grep -qx "$image_profile" \
    "$data_root/etc-cardputerzero/image-profile"
grep -qx "$access_profile" \
    "$data_root/etc-cardputerzero/access-profile"
for path in cardputerzero etc-cardputerzero extrausers home \
    network-connections network-state ssh; do
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
for database in passwd group shadow gshadow; do
    if [[ ! -f $data_root/extrausers/$database ||
          -L $data_root/extrausers/$database ]]; then
        echo "error: persistent owner database is invalid: $database" >&2
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

echo "PASS built rootfs and initramfs profile: $image_profile"
