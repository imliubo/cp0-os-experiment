#!/bin/bash -e

image_profile=$(cat "${STAGE_DIR}/image-profile")
case "$image_profile" in
    product | recovery) ;;
    *)
        echo "error: invalid CardputerZero image profile: $image_profile" >&2
        exit 1
        ;;
esac

source "${STAGE_DIR}/01-compositor/weston.env"

shell_source="${ROOTFS_DIR}/tmp/cardputerzero-system-shell"
policy_source="${ROOTFS_DIR}/tmp/cardputerzero-policy"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/cp0_ui.h" \
    "${shell_source}/cp0_ui.h"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/cp0_json.h" \
    "${shell_source}/cp0_json.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_appd_client.h" \
    "${shell_source}/cp0_appd_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_audio_settings_client.h" \
    "${shell_source}/cp0_audio_settings_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_connectivity_client.h" \
    "${shell_source}/cp0_connectivity_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_provision_client.h" \
    "${shell_source}/cp0_provision_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_shell_settings.h" \
    "${shell_source}/cp0_shell_settings.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_display_client.h" \
    "${shell_source}/cp0_display_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_developer_client.h" \
    "${shell_source}/cp0_developer_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_power_client.h" \
    "${shell_source}/cp0_power_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_screenshot_store.h" \
    "${shell_source}/cp0_screenshot_store.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_store_client.h" \
    "${shell_source}/cp0_store_client.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/cp0_system_info.h" \
    "${shell_source}/cp0_system_info.h"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/ui.c" \
    "${shell_source}/ui.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/json.c" \
    "${shell_source}/json.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/appd_client.c" \
    "${shell_source}/appd_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/audio_settings_client.c" \
    "${shell_source}/audio_settings_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/connectivity_client.c" \
    "${shell_source}/connectivity_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/provision_client.c" \
    "${shell_source}/provision_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/shell_settings.c" \
    "${shell_source}/shell_settings.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/display_client.c" \
    "${shell_source}/display_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/developer_client.c" \
    "${shell_source}/developer_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/power_client.c" \
    "${shell_source}/power_client.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/system-shell/screenshot_store.c" \
    "${shell_source}/screenshot_store.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/store_client.c" \
    "${shell_source}/store_client.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/system_info.c" \
    "${shell_source}/system_info.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/system-shell/main.c" \
    "${shell_source}/main.c"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/policy/cardputerzero-policy.c" \
    "${policy_source}/cardputerzero-policy.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/policy/esc-gesture.c" \
    "${policy_source}/esc-gesture.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/policy/esc-gesture.h" \
    "${policy_source}/esc-gesture.h"
install -D -m 0644 "${STAGE_DIR}/01-compositor/policy/overlay-state.c" \
    "${policy_source}/overlay-state.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/policy/overlay-state.h" \
    "${policy_source}/overlay-state.h"
install -D -m 0644 "${STAGE_DIR}/01-compositor/policy/wake-key.c" \
    "${policy_source}/wake-key.c"
install -D -m 0644 "${STAGE_DIR}/01-compositor/policy/wake-key.h" \
    "${policy_source}/wake-key.h"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/policy/cardputerzero-system-shell-v1.xml" \
    "${policy_source}/cardputerzero-system-shell-v1.xml"

on_chroot <<CHROOT
set -e

build_deps="
    build-essential
    git
    libcairo2-dev
    libdisplay-info-dev
    libdrm-dev
    libevdev-dev
    libinput-dev
    libpixman-1-dev
    libpng-dev
    libseat-dev
    libudev-dev
    libwayland-dev
    libxkbcommon-dev
    meson
    ninja-build
    pkg-config
    wayland-protocols
"

# pi-gen installs parts of the compiler toolchain before this stage and marks
# them as manually installed. Purge them explicitly after the Weston build;
# autoremove alone will otherwise leave them in the exported image.
toolchain_deps="
    binutils
    binutils-aarch64-linux-gnu
    binutils-common
    cpp
    cpp-14
    cpp-14-aarch64-linux-gnu
    cpp-aarch64-linux-gnu
    dpkg-dev
    g++
    g++-14
    g++-14-aarch64-linux-gnu
    g++-aarch64-linux-gnu
    gcc
    gcc-14
    gcc-14-aarch64-linux-gnu
    gcc-aarch64-linux-gnu
    libc-dev-bin
    libc6-dev
    libdpkg-perl
    libstdc++-14-dev
    linux-libc-dev
    make
"

apt-get update
apt-get install -y --no-install-recommends \$build_deps

rm -rf /tmp/cardputerzero-weston /tmp/cardputerzero-weston-build \
    /tmp/cardputerzero-weston-install
http_proxy="$APT_PROXY" https_proxy="$APT_PROXY" \
    git clone --no-checkout "${WESTON_REPOSITORY}" /tmp/cardputerzero-weston
git -C /tmp/cardputerzero-weston checkout "${WESTON_COMMIT}"
test "\$(git -C /tmp/cardputerzero-weston rev-parse HEAD)" = "${WESTON_COMMIT}"

meson setup /tmp/cardputerzero-weston-build /tmp/cardputerzero-weston \
    --prefix=/usr \
    --libdir=lib/aarch64-linux-gnu \
    -Dbackend-drm=true \
    -Dbackend-drm-screencast-vaapi=false \
    -Dbackend-headless=true \
    -Dbackend-pipewire=false \
    -Dbackend-rdp=false \
    -Dscreenshare=false \
    -Dbackend-vnc=false \
    -Dbackend-wayland=false \
    -Dbackend-x11=false \
    -Dbackend-default=drm \
    -Drenderer-gl=false \
    -Dxwayland=false \
    -Dsystemd=false \
    -Dremoting=false \
    -Dpipewire=false \
    -Dshell-desktop=false \
    -Dshell-fullscreen=false \
    -Dshell-ivi=false \
    -Dshell-kiosk=true \
    -Dcolor-management-lcms=false \
    -Dimage-jpeg=false \
    -Dimage-webp=false \
    -Dtools=[] \
    -Ddemo-clients=false \
    -Dsimple-clients=shm \
    -Dresize-pool=false \
    -Dwcap-decode=false \
    -Dtests=false \
    -Ddoc=false
meson compile -C /tmp/cardputerzero-weston-build

wayland-scanner client-header \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-v1.xml \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-client-protocol.h
wayland-scanner server-header \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-v1.xml \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-server-protocol.h
wayland-scanner private-code \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-v1.xml \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-protocol.c

cc -std=c11 -Os -Wall -Wextra -Werror \
    -I/tmp/cardputerzero-system-shell \
    -I/tmp/cardputerzero-policy \
    -I/tmp/cardputerzero-weston-build/protocol \
    /tmp/cardputerzero-system-shell/main.c \
    /tmp/cardputerzero-system-shell/ui.c \
    /tmp/cardputerzero-system-shell/screenshot_store.c \
    /tmp/cardputerzero-system-shell/json.c \
    /tmp/cardputerzero-system-shell/appd_client.c \
    /tmp/cardputerzero-system-shell/audio_settings_client.c \
    /tmp/cardputerzero-system-shell/connectivity_client.c \
    /tmp/cardputerzero-system-shell/provision_client.c \
    /tmp/cardputerzero-system-shell/shell_settings.c \
    /tmp/cardputerzero-system-shell/display_client.c \
    /tmp/cardputerzero-system-shell/developer_client.c \
    /tmp/cardputerzero-system-shell/power_client.c \
    /tmp/cardputerzero-system-shell/store_client.c \
    /tmp/cardputerzero-system-shell/system_info.c \
    /tmp/cardputerzero-policy/overlay-state.c \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-protocol.c \
    /tmp/cardputerzero-weston-build/protocol/xdg-shell-protocol.c \
    /tmp/cardputerzero-weston-build/protocol/weston-output-capture-protocol.c \
    \$(pkg-config --cflags --libs wayland-client libpng libdrm xkbcommon) \
    -o /tmp/cardputerzero-system-shell/cardputerzero-system-shell

cc -std=c11 -Os -Wall -Wextra -Werror -fPIC -shared -Wl,-z,defs \
    -I/tmp/cardputerzero-policy \
    -I/tmp/cardputerzero-weston \
    -I/tmp/cardputerzero-weston/include \
    -I/tmp/cardputerzero-weston-build \
    /tmp/cardputerzero-policy/cardputerzero-policy.c \
    /tmp/cardputerzero-policy/esc-gesture.c \
    /tmp/cardputerzero-policy/overlay-state.c \
    /tmp/cardputerzero-policy/wake-key.c \
    /tmp/cardputerzero-policy/cardputerzero-system-shell-protocol.c \
    -L/tmp/cardputerzero-weston-build/libweston -lweston-14 \
    \$(pkg-config --cflags --libs pixman-1 wayland-server) \
    -o /tmp/cardputerzero-policy/cardputerzero-policy.so

DESTDIR=/tmp/cardputerzero-weston-install \
    meson install --strip -C /tmp/cardputerzero-weston-build

install -D -m 0755 /tmp/cardputerzero-weston-install/usr/bin/weston \
    /usr/bin/weston
install -D -m 0755 /tmp/cardputerzero-weston-install/usr/bin/weston-simple-shm \
    /usr/bin/weston-simple-shm
install -D -m 0755 \
    /tmp/cardputerzero-system-shell/cardputerzero-system-shell \
    /usr/bin/cardputerzero-system-shell
install -D -m 0755 \
    /tmp/cardputerzero-weston-install/usr/lib/aarch64-linux-gnu/libweston-14.so.0.0.2 \
    /usr/lib/aarch64-linux-gnu/libweston-14.so.0.0.2
ln -sfn libweston-14.so.0.0.2 /usr/lib/aarch64-linux-gnu/libweston-14.so.0
install -D -m 0755 \
    /tmp/cardputerzero-weston-install/usr/lib/aarch64-linux-gnu/libweston-14/drm-backend.so \
    /usr/lib/aarch64-linux-gnu/libweston-14/drm-backend.so
install -D -m 0755 \
    /tmp/cardputerzero-weston-install/usr/lib/aarch64-linux-gnu/libweston-14/headless-backend.so \
    /usr/lib/aarch64-linux-gnu/libweston-14/headless-backend.so
install -D -m 0755 \
    /tmp/cardputerzero-weston-install/usr/lib/aarch64-linux-gnu/weston/kiosk-shell.so \
    /usr/lib/aarch64-linux-gnu/weston/kiosk-shell.so
install -D -m 0755 /tmp/cardputerzero-policy/cardputerzero-policy.so \
    /usr/lib/aarch64-linux-gnu/weston/cardputerzero-policy.so
install -D -m 0755 \
    /tmp/cardputerzero-weston-install/usr/lib/aarch64-linux-gnu/weston/libexec_weston.so.0.0.0 \
    /usr/lib/aarch64-linux-gnu/weston/libexec_weston.so.0.0.0
ln -sfn libexec_weston.so.0.0.0 \
    /usr/lib/aarch64-linux-gnu/weston/libexec_weston.so.0

ldconfig
weston --version | grep -qx 'weston ${WESTON_VERSION}'
if ldd /usr/bin/weston | grep -q 'not found'; then
    echo 'Weston runtime dependency is missing' >&2
    exit 1
fi
if ldd /usr/lib/aarch64-linux-gnu/weston/cardputerzero-policy.so | \
    grep -q 'not found'; then
    echo 'CardputerZero policy runtime dependency is missing' >&2
    exit 1
fi
if ldd /usr/bin/cardputerzero-system-shell | grep -q 'not found'; then
    echo 'CardputerZero System Shell runtime dependency is missing' >&2
    exit 1
fi

rm -rf /tmp/cardputerzero-weston \
    /tmp/cardputerzero-weston-build \
    /tmp/cardputerzero-weston-install \
    /tmp/cardputerzero-system-shell \
    /tmp/cardputerzero-policy
apt-get purge -y \$build_deps \$toolchain_deps
apt-get autoremove -y --purge
apt-get clean
CHROOT

install -D -m 0644 "${STAGE_DIR}/01-compositor/files/weston.ini" \
    "${ROOTFS_DIR}/etc/xdg/weston/weston.ini"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/files/cardputerzero-compositor.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-compositor.service"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/files/cardputerzero-system-shell.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-system-shell.service"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/files/cardputerzero-recovery-console.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-recovery-console.service"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/files/cardputerzero-display-retry.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-display-retry.service"
install -D -m 0755 \
    "${STAGE_DIR}/01-compositor/files/cardputerzero-display-generator" \
    "${ROOTFS_DIR}/usr/lib/systemd/system-generators/cardputerzero-display-generator"
install -D -m 0755 "${STAGE_DIR}/01-compositor/files/start-compositor.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/start-compositor.sh"
install -D -m 0755 "${STAGE_DIR}/01-compositor/files/wait-wayland.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/wait-wayland.sh"
install -D -m 0755 "${STAGE_DIR}/01-compositor/files/unblank-display.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/unblank-display.sh"
install -D -m 0755 \
    "${STAGE_DIR}/01-compositor/files/retry-display-once.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/retry-display-once.sh"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/files/99-cardputerzero-systemd.rules" \
    "${ROOTFS_DIR}/usr/lib/udev/rules.d/99-cardputerzero-systemd.rules"

on_chroot <<'CHROOT'
set -e
if ! getent group cp0-wayland >/dev/null 2>&1; then
    groupadd --system cp0-wayland
fi
if ! id cp0-compositor >/dev/null 2>&1; then
    useradd --system --home-dir /nonexistent --shell /usr/sbin/nologin \
        --groups video,render,input cp0-compositor
else
    usermod -G video,render,input cp0-compositor
fi
if ! id cp0-shell >/dev/null 2>&1; then
    useradd --system --home-dir /nonexistent --shell /usr/sbin/nologin \
        --groups cp0-wayland cp0-shell
else
    usermod -G cp0-wayland cp0-shell
fi
CHROOT

on_chroot <<'CHROOT'
set -e
systemctl disable getty@tty1.service cardputerzero-compositor.service \
    cardputerzero-recovery-console.service 2>/dev/null || true
CHROOT

if [[ $image_profile == product ]]; then
    on_chroot <<'CHROOT'
set -e
systemctl enable seatd.service cardputerzero-display-retry.service
CHROOT
else
    on_chroot <<'CHROOT'
set -e
systemctl disable seatd.service 2>/dev/null || true
systemctl mask cardputerzero-compositor.service \
    cardputerzero-system-shell.service \
    cardputerzero-display-retry.service
CHROOT
fi
