#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
    echo "usage: device-install-compositor.sh /tmp/cp0-<deployment>" >&2
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

for file in cardputerzero-system-shell cardputerzero-policy.so \
    cardputerzero-app-runtime cardputerzero-compositor.service \
    cardputerzero-display-generator \
    cardputerzero-display-retry.service retry-display-once.sh \
    unblank-display.sh \
    cardputerzero-recovery-console.service \
    cardputerzero-system-shell.service; do
    if [ ! -f "$staging/$file" ] || [ -L "$staging/$file" ]; then
        echo "error: invalid staged file: $file" >&2
        exit 1
    fi
done

if ! getent group cp0-display >/dev/null 2>&1; then
    groupadd --system cp0-display
fi
usermod -a -G cp0-display cp0-compositor
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-display.conf

if [ -e /var/lib/cardputerzero/registry/recovery-mode ]; then
    echo "error: disable recovery mode before compositor deployment" >&2
    exit 1
fi

if systemctl --quiet is-active 'cardputerzero-app-*.service'; then
    echo "error: stop the foreground application before compositor deployment" >&2
    exit 1
fi

for group in cp0-control cp0-display-control cp0-audio-control cp0-connectivity-control; do
    if ! getent group "$group" >/dev/null 2>&1; then
        groupadd --system "$group"
    fi
    usermod -a -G "$group" cp0-shell
done

systemctl stop cardputerzero-compositor.service
install -o root -g root -m 0644 \
    "$staging/cardputerzero-compositor.service" \
    /etc/systemd/system/cardputerzero-compositor.service
install -o root -g root -m 0644 \
    "$staging/cardputerzero-system-shell.service" \
    /etc/systemd/system/cardputerzero-system-shell.service
install -o root -g root -m 0644 \
    "$staging/cardputerzero-recovery-console.service" \
    /etc/systemd/system/cardputerzero-recovery-console.service
install -o root -g root -m 0644 \
    "$staging/cardputerzero-display-retry.service" \
    /etc/systemd/system/cardputerzero-display-retry.service
install -o root -g root -m 0755 \
    "$staging/retry-display-once.sh" \
    /usr/libexec/cardputerzero/retry-display-once.sh
install -o root -g root -m 0755 \
    "$staging/unblank-display.sh" \
    /usr/libexec/cardputerzero/unblank-display.sh
install -D -o root -g root -m 0755 \
    "$staging/cardputerzero-display-generator" \
    /usr/lib/systemd/system-generators/cardputerzero-display-generator
install -o root -g root -m 0755 "$staging/cardputerzero-system-shell" \
    /usr/bin/cardputerzero-system-shell
install -o root -g root -m 0755 "$staging/cardputerzero-policy.so" \
    /usr/lib/aarch64-linux-gnu/weston/cardputerzero-policy.so
install -o root -g root -m 0755 "$staging/cardputerzero-app-runtime" \
    /usr/libexec/cardputerzero/app-runtime
systemctl disable getty@tty1.service cardputerzero-compositor.service \
    cardputerzero-recovery-console.service 2>/dev/null || true
systemctl daemon-reload
systemctl start cardputerzero-compositor.service

wait_active()
{
    unit=$1
    attempt=0
    while [ "$attempt" -lt 40 ]; do
        if systemctl is-active --quiet "$unit"; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 0.25
    done
    echo "error: $unit did not become active" >&2
    return 1
}

wait_active cardputerzero-compositor.service
wait_active cardputerzero-system-shell.service
sha256sum \
    /usr/bin/cardputerzero-system-shell \
    /usr/lib/aarch64-linux-gnu/weston/cardputerzero-policy.so \
    /usr/libexec/cardputerzero/app-runtime
