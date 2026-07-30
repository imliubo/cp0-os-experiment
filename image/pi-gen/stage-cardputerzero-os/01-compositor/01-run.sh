#!/bin/bash -e

source "${STAGE_DIR}/01-compositor/weston.env"

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
DESTDIR=/tmp/cardputerzero-weston-install \
    meson install --strip -C /tmp/cardputerzero-weston-build

install -D -m 0755 /tmp/cardputerzero-weston-install/usr/bin/weston \
    /usr/bin/weston
install -D -m 0755 /tmp/cardputerzero-weston-install/usr/bin/weston-simple-shm \
    /usr/bin/weston-simple-shm
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

rm -rf /tmp/cardputerzero-weston \
    /tmp/cardputerzero-weston-build \
    /tmp/cardputerzero-weston-install
apt-get purge -y \$build_deps \$toolchain_deps
apt-get autoremove -y --purge
apt-get clean
CHROOT

install -D -m 0644 "${STAGE_DIR}/01-compositor/files/weston.ini" \
    "${ROOTFS_DIR}/etc/xdg/weston/weston.ini"
install -D -m 0644 \
    "${STAGE_DIR}/01-compositor/files/cardputerzero-compositor.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-compositor.service"
install -D -m 0755 "${STAGE_DIR}/01-compositor/files/start-compositor.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/start-compositor.sh"

on_chroot <<'CHROOT'
set -e
if ! id cp0-shell >/dev/null 2>&1; then
    useradd --system --home-dir /nonexistent --shell /usr/sbin/nologin \
        --groups video,render,input cp0-shell
fi
systemctl enable seatd.service
systemctl disable cardputerzero-compositor.service 2>/dev/null || true
CHROOT
