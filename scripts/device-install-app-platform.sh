#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: device-install-app-platform.sh /tmp/cp0-<deployment>" >&2
    exit 2
fi

staging=$1
case "$staging" in
    /tmp/cp0-*) ;;
    *)
        echo "error: staging directory must be below /tmp/cp0-*" >&2
        exit 2
        ;;
esac

for file in cp0-appd cp0-networkd cp0ctl cardputerzero-app-runtime app.json hello-card.wasm \
    cardputerzero-appd.service cardputerzero-networkd.service \
    cardputerzero-networkd.socket; do
    if [ ! -f "$staging/$file" ] || [ -L "$staging/$file" ]; then
        echo "error: invalid staged file: $file" >&2
        exit 1
    fi
done

systemctl stop cardputerzero-app-20000.service 2>/dev/null || :
systemctl stop cardputerzero-appd.service
systemctl stop cardputerzero-networkd.service 2>/dev/null || :
if ! getent group cp0-network >/dev/null 2>&1; then
    groupadd --system cp0-network
fi
if ! id cp0-network >/dev/null 2>&1; then
    useradd --system --gid cp0-network --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-network
fi
install -o root -g root -m 0755 "$staging/cp0-appd" \
    /usr/libexec/cardputerzero/cp0-appd
install -o root -g root -m 0755 "$staging/cp0-networkd" \
    /usr/libexec/cardputerzero/cp0-networkd
install -o root -g root -m 0755 "$staging/cp0ctl" /usr/bin/cp0ctl
install -o root -g root -m 0755 "$staging/cardputerzero-app-runtime" \
    /usr/libexec/cardputerzero/app-runtime
install -o root -g root -m 0644 "$staging/hello-card.wasm" \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm
install -o root -g root -m 0644 "$staging/app.json" \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/app.json
install -o root -g root -m 0644 "$staging/cardputerzero-appd.service" \
    /etc/systemd/system/cardputerzero-appd.service
install -o root -g root -m 0644 "$staging/cardputerzero-networkd.service" \
    /etc/systemd/system/cardputerzero-networkd.service
install -o root -g root -m 0644 "$staging/cardputerzero-networkd.socket" \
    /etc/systemd/system/cardputerzero-networkd.socket
systemctl daemon-reload
systemctl enable --now cardputerzero-networkd.socket
systemctl start cardputerzero-appd.service

systemctl is-active --quiet cardputerzero-appd.service
systemctl is-active --quiet cardputerzero-networkd.socket
systemctl is-active --quiet cardputerzero-compositor.service
systemctl is-active --quiet cardputerzero-system-shell.service
sha256sum \
    /usr/libexec/cardputerzero/cp0-appd \
    /usr/libexec/cardputerzero/cp0-networkd \
    /usr/bin/cp0ctl \
    /usr/libexec/cardputerzero/app-runtime \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm
