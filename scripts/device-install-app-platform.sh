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

for file in cp0-appd cp0-audiod cp0-camerad cp0-connectivityd cp0-displayd cp0-documentd cp0-gpiod cp0-networkd cp0-radiod cp0-storaged cp0-stored cp0-usb-mediad cp0ctl cardputerzero-app-runtime app.json hello-card.wasm camera-app.json camera.wasm \
    cardputerzero-appd.service cardputerzero-documentd.service \
    cardputerzero-appd.socket cardputerzero-broker.socket \
    cardputerzero-appd.conf \
    cardputerzero-documentd.socket cardputerzero-networkd.service \
    cardputerzero-networkd.socket cardputerzero-audiod.service \
    cardputerzero-audiod.socket cardputerzero-camerad.service \
    cardputerzero-camerad.socket cardputerzero-connectivityd.service \
    cardputerzero-connectivityd.socket cardputerzero-connectivity.conf \
    cardputerzero-displayd.service \
    cardputerzero-displayd.socket cardputerzero-display.conf \
    cardputerzero-gpiod.service \
    cardputerzero-gpiod.socket cardputerzero-gpio.conf \
    cardputerzero-radiod.service cardputerzero-radiod.socket lora.conf \
    cardputerzero-storaged.service cardputerzero-storaged.socket \
    cardputerzero-stored.service cardputerzero-stored.socket \
    cardputerzero-usb-mediad.service cardputerzero-usb-mediad.socket \
    cardputerzero-usb-media.conf cardputerzero-usb-media.modules \
    cardputerzero-storage.conf cardputerzero-store.conf store.conf \
    device-policy.json \
    cardputerzero-trust.conf \
    device-capability-acceptance.sh device-core-recovery.sh \
    device-factory-acceptance.sh \
    device-performance-acceptance.sh device-stability-monitor.sh \
    device-store-acceptance.sh device-smoke.sh; do
    if [ ! -f "$staging/$file" ] || [ -L "$staging/$file" ]; then
        echo "error: invalid staged file: $file" >&2
        exit 1
    fi
done

systemctl stop cardputerzero-system-shell.service 2>/dev/null || :
systemctl stop 'cardputerzero-app-*.service' 2>/dev/null || :
systemctl stop cardputerzero-appd.service 2>/dev/null || :
systemctl stop cardputerzero-appd.socket cardputerzero-broker.socket \
    cardputerzero-networkd.socket cardputerzero-documentd.socket \
    cardputerzero-audiod.socket cardputerzero-camerad.socket \
    cardputerzero-connectivityd.socket \
    cardputerzero-displayd.socket cardputerzero-gpiod.socket \
    cardputerzero-radiod.socket cardputerzero-storaged.socket \
    cardputerzero-stored.socket cardputerzero-usb-mediad.socket \
    2>/dev/null || :
systemctl stop cardputerzero-networkd.service 2>/dev/null || :
systemctl stop cardputerzero-documentd.service 2>/dev/null || :
systemctl stop cardputerzero-audiod.service 2>/dev/null || :
systemctl stop cardputerzero-camerad.service 2>/dev/null || :
systemctl stop cardputerzero-connectivityd.service 2>/dev/null || :
systemctl stop cardputerzero-displayd.service 2>/dev/null || :
systemctl stop cardputerzero-gpiod.service 2>/dev/null || :
systemctl stop cardputerzero-radiod.service 2>/dev/null || :
systemctl stop cardputerzero-storaged.service 2>/dev/null || :
systemctl stop cardputerzero-stored.service 2>/dev/null || :
systemctl stop cardputerzero-usb-mediad.service 2>/dev/null || :
if ! getent group cp0-control >/dev/null 2>&1; then
    groupadd --system cp0-control
fi
usermod -a -G cp0-control cp0-shell
if ! getent group cp0-display-control >/dev/null 2>&1; then
    groupadd --system cp0-display-control
fi
usermod -a -G cp0-display-control cp0-shell
if ! getent group cp0-audio-control >/dev/null 2>&1; then
    groupadd --system cp0-audio-control
fi
usermod -a -G cp0-audio-control cp0-shell
if ! getent group cp0-connectivity-control >/dev/null 2>&1; then
    groupadd --system cp0-connectivity-control
fi
usermod -a -G cp0-connectivity-control cp0-shell
if ! getent group cp0-usb-media-control >/dev/null 2>&1; then
    groupadd --system cp0-usb-media-control
fi
usermod -a -G cp0-usb-media-control cp0-shell
if ! getent group cp0-display >/dev/null 2>&1; then
    groupadd --system cp0-display
fi
if ! id cp0-display >/dev/null 2>&1; then
    useradd --system --gid cp0-display --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-display
fi
if ! getent group cp0-store >/dev/null 2>&1; then
    groupadd --system cp0-store
fi
if ! id cp0-store >/dev/null 2>&1; then
    useradd --system --gid cp0-store --groups cp0-control \
        --home-dir /nonexistent --shell /usr/sbin/nologin cp0-store
else
    usermod -a -G cp0-control cp0-store
fi
if ! getent group cp0-network >/dev/null 2>&1; then
    groupadd --system cp0-network
fi
if ! id cp0-network >/dev/null 2>&1; then
    useradd --system --gid cp0-network --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-network
fi
if ! getent group cp0-document >/dev/null 2>&1; then
    groupadd --system cp0-document
fi
if ! id cp0-document >/dev/null 2>&1; then
    useradd --system --gid cp0-document --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-document
fi
if ! getent group cp0-audio >/dev/null 2>&1; then
    groupadd --system cp0-audio
fi
if ! id cp0-audio >/dev/null 2>&1; then
    useradd --system --gid cp0-audio --groups audio --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-audio
fi
if ! getent group cp0-camera >/dev/null 2>&1; then
    groupadd --system cp0-camera
fi
if ! id cp0-camera >/dev/null 2>&1; then
    useradd --system --gid cp0-camera --groups video --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-camera
fi
if ! getent group cp0-gpio >/dev/null 2>&1; then
    groupadd --system cp0-gpio
fi
if ! id cp0-gpio >/dev/null 2>&1; then
    useradd --system --gid cp0-gpio --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-gpio
fi
if ! getent group cp0-radio >/dev/null 2>&1; then
    groupadd --system cp0-radio
fi
if ! id cp0-radio >/dev/null 2>&1; then
    useradd --system --gid cp0-radio --groups spi --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-radio
fi
if ! getent group cp0-storage >/dev/null 2>&1; then
    groupadd --system cp0-storage
fi
if ! id cp0-storage >/dev/null 2>&1; then
    useradd --system --gid cp0-storage --home-dir /nonexistent \
        --shell /usr/sbin/nologin cp0-storage
fi
app_account_id=20000
while [ "$app_account_id" -le 20063 ]; do
    app_account="cp0-app-$app_account_id"
    if ! getent group "$app_account" >/dev/null 2>&1; then
        groupadd --system --gid "$app_account_id" "$app_account"
    fi
    if ! id "$app_account" >/dev/null 2>&1; then
        useradd --system --uid "$app_account_id" --gid "$app_account_id" \
            --home-dir /nonexistent --shell /usr/sbin/nologin "$app_account"
    fi
    app_account_id=$((app_account_id + 1))
done
install -d -o cp0-storage -g cp0-storage -m 0700 \
    /var/lib/cardputerzero/data
chown -R cp0-storage:cp0-storage /var/lib/cardputerzero/data
chmod 0700 /var/lib/cardputerzero/data
install -d -o cp0-document -g cp0-document -m 0750 \
    /var/lib/cardputerzero/documents
if [ ! -e /var/lib/cardputerzero/documents/welcome.txt ]; then
    printf '%s\n' 'CardputerZero Document Portal is ready.' \
        >/var/lib/cardputerzero/documents/welcome.txt
    chown cp0-document:cp0-document \
        /var/lib/cardputerzero/documents/welcome.txt
    chmod 0640 /var/lib/cardputerzero/documents/welcome.txt
fi
install -o root -g root -m 0755 "$staging/cp0-appd" \
    /usr/libexec/cardputerzero/cp0-appd
install -o root -g root -m 0755 "$staging/cp0-networkd" \
    /usr/libexec/cardputerzero/cp0-networkd
install -o root -g root -m 0755 "$staging/cp0-documentd" \
    /usr/libexec/cardputerzero/cp0-documentd
install -o root -g root -m 0755 "$staging/cp0-audiod" \
    /usr/libexec/cardputerzero/cp0-audiod
install -o root -g root -m 0755 "$staging/cp0-camerad" \
    /usr/libexec/cardputerzero/cp0-camerad
install -o root -g root -m 0755 "$staging/cp0-connectivityd" \
    /usr/libexec/cardputerzero/cp0-connectivityd
install -o root -g root -m 0755 "$staging/cp0-displayd" \
    /usr/libexec/cardputerzero/cp0-displayd
install -o root -g root -m 0755 "$staging/cp0-gpiod" \
    /usr/libexec/cardputerzero/cp0-gpiod
install -o root -g root -m 0755 "$staging/cp0-radiod" \
    /usr/libexec/cardputerzero/cp0-radiod
install -o root -g root -m 0755 "$staging/cp0-storaged" \
    /usr/libexec/cardputerzero/cp0-storaged
install -o root -g root -m 0755 "$staging/cp0-stored" \
    /usr/libexec/cardputerzero/cp0-stored
install -o root -g root -m 0755 "$staging/cp0-usb-mediad" \
    /usr/libexec/cardputerzero/cp0-usb-mediad
install -o root -g root -m 0755 "$staging/cp0ctl" /usr/bin/cp0ctl
install -o root -g root -m 0755 "$staging/cardputerzero-app-runtime" \
    /usr/libexec/cardputerzero/app-runtime
install -o root -g root -m 0644 "$staging/hello-card.wasm" \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm
install -o root -g root -m 0644 "$staging/app.json" \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/app.json
install -D -o root -g root -m 0644 "$staging/camera.wasm" \
    /var/lib/cardputerzero/apps/dev.cardputerzero.camera/0.1.0/bin/camera.wasm
install -D -o root -g root -m 0644 "$staging/camera-app.json" \
    /var/lib/cardputerzero/apps/dev.cardputerzero.camera/0.1.0/app.json
install -o root -g root -m 0644 "$staging/cardputerzero-appd.service" \
    /etc/systemd/system/cardputerzero-appd.service
install -o root -g root -m 0644 "$staging/cardputerzero-appd.socket" \
    /etc/systemd/system/cardputerzero-appd.socket
install -o root -g root -m 0644 "$staging/cardputerzero-broker.socket" \
    /etc/systemd/system/cardputerzero-broker.socket
install -o root -g root -m 0644 "$staging/cardputerzero-networkd.service" \
    /etc/systemd/system/cardputerzero-networkd.service
install -o root -g root -m 0644 "$staging/cardputerzero-networkd.socket" \
    /etc/systemd/system/cardputerzero-networkd.socket
install -o root -g root -m 0644 "$staging/cardputerzero-documentd.service" \
    /etc/systemd/system/cardputerzero-documentd.service
install -o root -g root -m 0644 "$staging/cardputerzero-documentd.socket" \
    /etc/systemd/system/cardputerzero-documentd.socket
install -o root -g root -m 0644 "$staging/cardputerzero-audiod.service" \
    /etc/systemd/system/cardputerzero-audiod.service
install -o root -g root -m 0644 "$staging/cardputerzero-audiod.socket" \
    /etc/systemd/system/cardputerzero-audiod.socket
install -o root -g root -m 0644 "$staging/cardputerzero-camerad.service" \
    /etc/systemd/system/cardputerzero-camerad.service
install -o root -g root -m 0644 "$staging/cardputerzero-camerad.socket" \
    /etc/systemd/system/cardputerzero-camerad.socket
install -o root -g root -m 0644 "$staging/cardputerzero-connectivityd.service" \
    /etc/systemd/system/cardputerzero-connectivityd.service
install -o root -g root -m 0644 "$staging/cardputerzero-connectivityd.socket" \
    /etc/systemd/system/cardputerzero-connectivityd.socket
install -o root -g root -m 0644 "$staging/cardputerzero-displayd.service" \
    /etc/systemd/system/cardputerzero-displayd.service
install -o root -g root -m 0644 "$staging/cardputerzero-displayd.socket" \
    /etc/systemd/system/cardputerzero-displayd.socket
install -o root -g root -m 0644 "$staging/cardputerzero-gpiod.service" \
    /etc/systemd/system/cardputerzero-gpiod.service
install -o root -g root -m 0644 "$staging/cardputerzero-gpiod.socket" \
    /etc/systemd/system/cardputerzero-gpiod.socket
install -o root -g root -m 0644 "$staging/cardputerzero-radiod.service" \
    /etc/systemd/system/cardputerzero-radiod.service
install -o root -g root -m 0644 "$staging/cardputerzero-radiod.socket" \
    /etc/systemd/system/cardputerzero-radiod.socket
install -o root -g root -m 0644 "$staging/cardputerzero-storaged.service" \
    /etc/systemd/system/cardputerzero-storaged.service
install -o root -g root -m 0644 "$staging/cardputerzero-storaged.socket" \
    /etc/systemd/system/cardputerzero-storaged.socket
install -o root -g root -m 0644 "$staging/cardputerzero-stored.service" \
    /etc/systemd/system/cardputerzero-stored.service
install -o root -g root -m 0644 "$staging/cardputerzero-stored.socket" \
    /etc/systemd/system/cardputerzero-stored.socket
install -o root -g root -m 0644 "$staging/cardputerzero-usb-mediad.service" \
    /etc/systemd/system/cardputerzero-usb-mediad.service
install -o root -g root -m 0644 "$staging/cardputerzero-usb-mediad.socket" \
    /etc/systemd/system/cardputerzero-usb-mediad.socket
install -o root -g root -m 0644 "$staging/cardputerzero-storage.conf" \
    /etc/tmpfiles.d/cardputerzero-storage.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-storage.conf
install -o root -g root -m 0644 "$staging/cardputerzero-store.conf" \
    /etc/tmpfiles.d/cardputerzero-store.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-store.conf
install -o root -g root -m 0644 "$staging/cardputerzero-usb-media.conf" \
    /etc/tmpfiles.d/cardputerzero-usb-media.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-usb-media.conf
install -o root -g root -m 0644 "$staging/cardputerzero-usb-media.modules" \
    /etc/modules-load.d/cardputerzero-usb-media.conf
install -o root -g root -m 0644 "$staging/cardputerzero-trust.conf" \
    /etc/tmpfiles.d/cardputerzero-trust.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-trust.conf
install -o root -g root -m 0644 "$staging/cardputerzero-appd.conf" \
    /etc/tmpfiles.d/cardputerzero-appd.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-appd.conf
install -o root -g root -m 0644 "$staging/cardputerzero-display.conf" \
    /etc/tmpfiles.d/cardputerzero-display.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-display.conf
install -o root -g root -m 0644 "$staging/cardputerzero-connectivity.conf" \
    /etc/tmpfiles.d/cardputerzero-connectivity.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-connectivity.conf
install -D -o root -g root -m 0644 "$staging/lora.conf" \
    /etc/cardputerzero/lora.conf
if [ ! -e /etc/cardputerzero/store.conf ]; then
    install -D -o root -g root -m 0644 "$staging/store.conf" \
        /etc/cardputerzero/store.conf
fi
if [ ! -e /etc/cardputerzero/device-policy.json ]; then
    install -D -o root -g root -m 0644 "$staging/device-policy.json" \
        /etc/cardputerzero/device-policy.json
fi
install -o root -g root -m 0644 "$staging/cardputerzero-gpio.conf" \
    /etc/tmpfiles.d/cardputerzero-gpio.conf
systemd-tmpfiles --create /etc/tmpfiles.d/cardputerzero-gpio.conf
install -o root -g root -m 0755 "$staging/device-core-recovery.sh" \
    /usr/libexec/cardputerzero/device-core-recovery
install -o root -g root -m 0755 "$staging/device-capability-acceptance.sh" \
    /usr/libexec/cardputerzero/device-capability-acceptance
install -o root -g root -m 0755 "$staging/device-factory-acceptance.sh" \
    /usr/libexec/cardputerzero/device-factory-acceptance
install -o root -g root -m 0755 "$staging/device-performance-acceptance.sh" \
    /usr/libexec/cardputerzero/device-performance-acceptance
install -o root -g root -m 0755 "$staging/device-stability-monitor.sh" \
    /usr/libexec/cardputerzero/device-stability-monitor
install -o root -g root -m 0755 "$staging/device-store-acceptance.sh" \
    /usr/libexec/cardputerzero/device-store-acceptance
install -o root -g root -m 0755 "$staging/device-smoke.sh" \
    /usr/libexec/cardputerzero/device-smoke.sh
systemctl daemon-reload
modprobe libcomposite
systemctl enable --now cardputerzero-appd.socket cardputerzero-broker.socket
systemctl enable --now cardputerzero-networkd.socket
systemctl enable --now cardputerzero-documentd.socket
systemctl enable --now cardputerzero-audiod.socket
systemctl enable --now cardputerzero-camerad.socket
systemctl enable --now cardputerzero-connectivityd.socket
systemctl enable --now cardputerzero-displayd.socket
systemctl enable --now cardputerzero-gpiod.socket
systemctl enable --now cardputerzero-radiod.socket
systemctl enable --now cardputerzero-storaged.socket
systemctl enable --now cardputerzero-stored.socket
systemctl enable --now cardputerzero-usb-mediad.socket
systemctl start cardputerzero-appd.service
systemctl start cardputerzero-system-shell.service

systemctl is-active --quiet cardputerzero-appd.service
systemctl is-active --quiet cardputerzero-appd.socket
systemctl is-active --quiet cardputerzero-broker.socket
systemctl is-active --quiet cardputerzero-networkd.socket
systemctl is-active --quiet cardputerzero-documentd.socket
systemctl is-active --quiet cardputerzero-audiod.socket
systemctl is-active --quiet cardputerzero-camerad.socket
systemctl is-active --quiet cardputerzero-connectivityd.socket
systemctl is-active --quiet cardputerzero-displayd.socket
systemctl is-active --quiet cardputerzero-gpiod.socket
systemctl is-active --quiet cardputerzero-radiod.socket
systemctl is-active --quiet cardputerzero-storaged.socket
systemctl is-active --quiet cardputerzero-stored.socket
systemctl is-active --quiet cardputerzero-usb-mediad.socket
systemctl is-active --quiet cardputerzero-compositor.service
systemctl is-active --quiet cardputerzero-system-shell.service
sha256sum \
    /usr/libexec/cardputerzero/cp0-appd \
    /usr/libexec/cardputerzero/cp0-documentd \
    /usr/libexec/cardputerzero/cp0-audiod \
    /usr/libexec/cardputerzero/cp0-camerad \
    /usr/libexec/cardputerzero/cp0-connectivityd \
    /usr/libexec/cardputerzero/cp0-displayd \
    /usr/libexec/cardputerzero/cp0-gpiod \
    /usr/libexec/cardputerzero/cp0-radiod \
    /usr/libexec/cardputerzero/cp0-storaged \
    /usr/libexec/cardputerzero/cp0-stored \
    /usr/libexec/cardputerzero/cp0-usb-mediad \
    /usr/libexec/cardputerzero/cp0-networkd \
    /usr/bin/cp0ctl \
    /usr/libexec/cardputerzero/app-runtime \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm \
    /var/lib/cardputerzero/apps/dev.cardputerzero.camera/0.1.0/bin/camera.wasm
