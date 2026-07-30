#!/bin/bash -e

payload="${STAGE_DIR}/02-app-platform/payload"
hello_root="${ROOTFS_DIR}/var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0"

install -D -m 0755 "${payload}/cp0-appd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-appd"
install -D -m 0755 "${payload}/cp0-networkd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-networkd"
install -D -m 0755 "${payload}/cp0ctl" \
    "${ROOTFS_DIR}/usr/bin/cp0ctl"
install -D -m 0755 "${payload}/cardputerzero-app-runtime" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/app-runtime"
install -D -m 0644 "${payload}/systemd/cardputerzero-appd.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-appd.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-appd.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-appd.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-broker.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-broker.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-networkd.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-networkd.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-networkd.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-networkd.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-appd.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-appd.conf"
install -D -m 0755 "${payload}/diagnostics/device-core-recovery.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-core-recovery"
install -D -m 0755 "${payload}/diagnostics/device-stability-monitor.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-stability-monitor"
install -D -m 0644 "${payload}/hello/app.json" "${hello_root}/app.json"
install -D -m 0644 "${payload}/hello/bin/hello-card.wasm" \
    "${hello_root}/bin/hello-card.wasm"

on_chroot <<'CHROOT'
set -e

if ! getent group cp0-control >/dev/null 2>&1; then
    groupadd --system cp0-control
fi
usermod -a -G cp0-control cp0-shell
if ! getent group cp0-network >/dev/null 2>&1; then
    groupadd --system cp0-network
fi
if ! id cp0-network >/dev/null 2>&1; then
    useradd --system --gid cp0-network --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-network
fi
if ! getent group cp0-app-20000 >/dev/null 2>&1; then
    groupadd --system --gid 20000 cp0-app-20000
fi
if ! id cp0-app-20000 >/dev/null 2>&1; then
    useradd --system --uid 20000 --gid 20000 --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-app-20000
fi

install -d -o root -g root -m 0755 \
    /var/lib/cardputerzero \
    /var/lib/cardputerzero/apps \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0 \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin \
    /var/lib/cardputerzero/data
install -d -o root -g root -m 0700 /var/lib/cardputerzero/registry
install -d -o cp0-app-20000 -g cp0-app-20000 -m 0700 \
    /var/lib/cardputerzero/data/dev.cardputerzero.hello
chown -R root:root /var/lib/cardputerzero/apps/dev.cardputerzero.hello
chmod -R go-w /var/lib/cardputerzero/apps/dev.cardputerzero.hello
/usr/libexec/cardputerzero/cp0-appd register-installed \
    dev.cardputerzero.hello 0.1.0

systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-appd.conf
systemctl enable cardputerzero-appd.socket cardputerzero-broker.socket \
    cardputerzero-networkd.socket
CHROOT
