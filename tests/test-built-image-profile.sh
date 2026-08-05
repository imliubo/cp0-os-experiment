#!/usr/bin/env bash
set -euo pipefail

if (($# != 1)); then
    echo "usage: $0 IMAGE_INFO" >&2
    exit 2
fi

image_info=$1
test -f "$image_info"

package_is_installed() {
    awk -v expected="$1" '
        $1 == "ii" {
            package = $2
            sub(/:.*/, "", package)
            if (package == expected) {
                found = 1
            }
        }
        END { exit !found }
    ' "$image_info"
}

for package in \
    apparmor bubblewrap fbset firmware-brcm80211 network-manager openssh-server \
    raspberrypi-sys-mods rpi-swap rpicam-apps-lite \
    linux-image-rpi-v8 \
    libcairo2 libdisplay-info2 libdrm2 libevdev2 libinput10 libpixman-1-0 \
    libpng16-16t64 libseat1 libwayland-client0 libwayland-server0 \
    libxkbcommon0 seatd xkb-data; do
    if ! package_is_installed "$package"; then
        echo "error: required package missing from image: $package" >&2
        exit 1
    fi
done

for package in \
    lightdm wayfire wf-panel-pi pcmanfm pcmanfm-qt packagekit \
    pipewire pipewire-pulse wireplumber libinput-tools weston xwayland \
    linux-image-rpi-2712 linux-headers-rpi-v8 linux-headers-rpi-2712 \
    binutils cpp gcc g++ dpkg-dev libc6-dev libstdc++-14-dev make \
    git meson ninja-build \
    libcairo2-dev libdisplay-info-dev libdrm-dev libevdev-dev libinput-dev \
    libpixman-1-dev libpng-dev libseat-dev libudev-dev libwayland-dev \
    libxkbcommon-dev wayland-protocols; do
    if package_is_installed "$package"; then
        echo "error: prohibited package remains in image: $package" >&2
        exit 1
    fi
done
