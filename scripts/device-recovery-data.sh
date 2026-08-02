#!/bin/bash
set -euo pipefail

mount_root=/run/cardputerzero-recovery-data
factory_bundle=/usr/share/cardputerzero/factory-data-v1.cp0backup
mounted=0

usage() {
    echo "usage: device-recovery-data verify BACKUP.cp0backup" >&2
    echo "       device-recovery-data backup DEVICE OUTPUT.cp0backup" >&2
    echo "       device-recovery-data restore DEVICE BACKUP.cp0backup RESTORE-CP0-DATA" >&2
    echo "       device-recovery-data factory-reset DEVICE RESET-CP0-DATA" >&2
    exit 2
}

cleanup() {
    if ((mounted == 1)); then
        umount "$mount_root" || true
    fi
}
trap cleanup EXIT

if (($# < 1)); then
    usage
fi
command=$1
shift

if [[ $command == verify ]]; then
    (($# == 1)) || usage
    exec /usr/bin/cp0-recovery verify "$1"
fi

if ((EUID != 0)); then
    echo "error: backup, restore and factory-reset must run as root" >&2
    exit 2
fi

image_profile=$(cat /etc/cardputerzero/image-profile 2>/dev/null || true)
root_type=$(findmnt -n -o FSTYPE / 2>/dev/null || true)
cmdline=" $(cat /proc/cmdline 2>/dev/null) "
case "$image_profile" in
    recovery)
        if [[ $root_type == overlay ]]; then
            echo "error: recovery image unexpectedly uses an overlay root" >&2
            exit 1
        fi
        ;;
    product)
        if [[ $cmdline == *" cp0.overlay_root=volatile "* ]] || [[ $root_type == overlay ]]; then
            echo "error: reboot the product lower-root maintenance profile first" >&2
            exit 1
        fi
        ;;
    *)
        echo "error: independent recovery or product lower-root maintenance is required" >&2
        exit 1
        ;;
esac

canonical_device() {
    local requested=$1 resolved root_source
    resolved=$(readlink -f -- "$requested")
    if [[ ! -b $resolved ]] || [[ $(lsblk -dnro TYPE "$resolved") != part ]] ||
        [[ $(lsblk -dnro PARTN "$resolved") != 3 ]]; then
        echo "error: recovery target must be a real partition 3 block device" >&2
        return 1
    fi
    if [[ $(blkid -s LABEL -o value "$resolved") != cp0-data ]] ||
        [[ $(blkid -s TYPE -o value "$resolved") != ext4 ]]; then
        echo "error: recovery target must be an ext4 filesystem labelled cp0-data" >&2
        return 1
    fi
    if findmnt -rn -S "$resolved" | grep -q .; then
        echo "error: cp0-data must be unmounted before recovery" >&2
        return 1
    fi
    root_source=$(findmnt -n -o SOURCE / 2>/dev/null || true)
    root_source=$(readlink -f -- "$root_source" 2>/dev/null || true)
    if [[ -n $root_source && $resolved == "$root_source" ]]; then
        echo "error: refusing to operate on the active root filesystem" >&2
        return 1
    fi
    printf '%s\n' "$resolved"
}

prepare_mount_root() {
    install -d -o root -g root -m 0700 "$mount_root"
    if mountpoint -q "$mount_root"; then
        echo "error: recovery mount point is already in use" >&2
        exit 1
    fi
}

mount_data() {
    local device=$1 access=$2
    prepare_mount_root
    mount -t ext4 -o "$access,nodev,nosuid,noexec,noatime" \
        "$device" "$mount_root"
    mounted=1
}

require_product_data() {
    if [[ $(cat "$mount_root/etc-cardputerzero/image-profile" 2>/dev/null || true) != product ]] ||
        [[ $(cat "$mount_root/layout-version" 2>/dev/null || true) != cp0-data-layout-v2 ]]; then
        echo "error: target is not a product cp0-data v2 filesystem" >&2
        exit 1
    fi
}

require_external_output() {
    local output=$1 parent target
    if [[ $output != /* ]] || [[ -e $output ]]; then
        echo "error: backup output must be a new absolute path" >&2
        return 1
    fi
    parent=$(readlink -f -- "$(dirname -- "$output")")
    target=$(findmnt -n -o TARGET --target "$parent" 2>/dev/null || true)
    if [[ -z $target || $target == / || $target == /boot/firmware ]] ||
        [[ $parent == "$mount_root" || $parent == "$mount_root/"* ]]; then
        echo "error: backup output must be on a separately mounted filesystem" >&2
        return 1
    fi
}

format_and_restore() {
    local device=$1 bundle=$2 verify_output features
    verify_output=$(/usr/bin/cp0-recovery verify "$bundle")
    if [[ $verify_output != *" profile=product" ]]; then
        echo "error: only a verified product backup can populate cp0-data" >&2
        exit 1
    fi
    features=^huge_file
    if grep -q 64bit /etc/mke2fs.conf; then
        features="^64bit,$features"
    fi
    mkfs.ext4 -F -L cp0-data -m 1 -O "$features" "$device" >/dev/null
    mount_data "$device" rw
    chmod 0700 "$mount_root"
    /usr/bin/cp0-recovery restore "$bundle" "$mount_root"
    sync -f "$mount_root"
    umount "$mount_root"
    mounted=0
    set +e
    e2fsck -pf "$device"
    check_status=$?
    set -e
    if ((check_status > 1)); then
        echo "error: restored cp0-data filesystem check failed: $check_status" >&2
        exit 1
    fi
    echo "PASS restored product cp0-data on $device"
}

case "$command" in
    backup)
        (($# == 2)) || usage
        device=$(canonical_device "$1")
        output=$2
        require_external_output "$output"
        mount_data "$device" ro
        require_product_data
        /usr/bin/cp0-recovery backup "$mount_root" "$output"
        sync -f "$(dirname -- "$output")"
        ;;
    restore)
        (($# == 3)) || usage
        device=$(canonical_device "$1")
        bundle=$2
        confirmation=$3
        if [[ $confirmation != RESTORE-CP0-DATA ]]; then
            echo "error: restore requires the exact confirmation RESTORE-CP0-DATA" >&2
            exit 2
        fi
        format_and_restore "$device" "$bundle"
        ;;
    factory-reset)
        (($# == 2)) || usage
        device=$(canonical_device "$1")
        confirmation=$2
        if [[ $image_profile != product ]]; then
            echo "error: factory-reset requires the product lower-root maintenance profile" >&2
            exit 1
        fi
        if [[ $confirmation != RESET-CP0-DATA ]]; then
            echo "error: factory-reset requires the exact confirmation RESET-CP0-DATA" >&2
            exit 2
        fi
        if [[ ! -f $factory_bundle ]] || [[ -L $factory_bundle ]]; then
            echo "error: trusted factory data bundle is unavailable" >&2
            exit 1
        fi
        format_and_restore "$device" "$factory_bundle"
        ;;
    *) usage ;;
esac
