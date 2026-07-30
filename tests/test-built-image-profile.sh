#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
    echo "usage: $0 IMAGE_INFO" >&2
    exit 2
fi

image_info=$1
test -f "$image_info"

package_is_installed() {
    grep -Eq "^ii[[:space:]]+$1([[:space:]:]|$)" "$image_info"
}

for package in \
    apparmor bubblewrap network-manager openssh-server rpi-swap \
    linux-image-rpi-v8; do
    if ! package_is_installed "$package"; then
        echo "error: required package missing from image: $package" >&2
        exit 1
    fi
done

for package in \
    lightdm wayfire wf-panel-pi pcmanfm pcmanfm-qt packagekit \
    pipewire pipewire-pulse wireplumber libinput-tools \
    linux-image-rpi-2712 linux-headers-rpi-v8 linux-headers-rpi-2712 \
    binutils cpp gcc dpkg-dev libc6-dev make; do
    if package_is_installed "$package"; then
        echo "error: prohibited package remains in image: $package" >&2
        exit 1
    fi
done
