#!/bin/bash -e

payload="${STAGE_DIR}/02-app-platform/payload"
hello_root="${ROOTFS_DIR}/var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0"
image_profile=$(cat "${STAGE_DIR}/image-profile")
access_profile=$(cat "${STAGE_DIR}/access-profile")
case "$image_profile" in
    product | recovery) ;;
    *)
        echo "error: invalid CardputerZero image profile: $image_profile" >&2
        exit 1
        ;;
esac
case "$access_profile" in
    development | production) ;;
    *)
        echo "error: invalid CardputerZero access profile: $access_profile" >&2
        exit 1
        ;;
esac

install -D -m 0755 "${payload}/cp0-appd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-appd"
install -D -m 0755 "${payload}/cp0-networkd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-networkd"
install -D -m 0755 "${payload}/cp0-provisiond" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-provisiond"
install -D -m 0755 "${payload}/cp0-documentd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-documentd"
install -D -m 0755 "${payload}/cp0-audiod" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-audiod"
install -D -m 0755 "${payload}/cp0-camerad" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-camerad"
install -D -m 0755 "${payload}/cp0-connectivityd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-connectivityd"
install -D -m 0755 "${payload}/cp0-displayd" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-displayd"
install -D -m 0755 "${payload}/cp0-gpiod" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-gpiod"
install -D -m 0755 "${payload}/cp0-radiod" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-radiod"
install -D -m 0755 "${payload}/cp0-recovery" \
    "${ROOTFS_DIR}/usr/bin/cp0-recovery"
install -D -m 0755 "${payload}/cp0-storaged" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-storaged"
install -D -m 0755 "${payload}/cp0-stored" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/cp0-stored"
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
install -D -m 0644 "${payload}/systemd/cardputerzero-documentd.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-documentd.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-documentd.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-documentd.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-audiod.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-audiod.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-audiod.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-audiod.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-camerad.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-camerad.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-camerad.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-camerad.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-connectivityd.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-connectivityd.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-connectivityd.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-connectivityd.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-provisiond.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-provisiond.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-provisiond.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-provisiond.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-provision-apply.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-provision-apply.service"
install -D -m 0755 "${payload}/systemd/cardputerzero-ssh-generator" \
    "${ROOTFS_DIR}/usr/lib/systemd/system-generators/cardputerzero-ssh-generator"
install -D -m 0644 "${payload}/systemd/cardputerzero-ssh-gate.conf" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/ssh.service.d/cardputerzero-gate.conf"
install -D -m 0600 "${payload}/systemd/cardputerzero-sshd.conf" \
    "${ROOTFS_DIR}/etc/ssh/sshd_config.d/40-cardputerzero-owner.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-displayd.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-displayd.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-displayd.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-displayd.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-gpiod.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-gpiod.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-gpiod.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-gpiod.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-radiod.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-radiod.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-radiod.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-radiod.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-storaged.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-storaged.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-storaged.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-storaged.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-stored.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-stored.service"
install -D -m 0644 "${payload}/systemd/cardputerzero-stored.socket" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-stored.socket"
install -D -m 0644 "${payload}/systemd/cardputerzero-gpio.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-gpio.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-display.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-display.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-connectivity.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-connectivity.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-provision.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-provision.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-appd.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-appd.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-storage.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-storage.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-trust.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-trust.conf"
install -D -m 0644 "${payload}/systemd/cardputerzero-store.conf" \
    "${ROOTFS_DIR}/usr/lib/tmpfiles.d/cardputerzero-store.conf"
install -d -o root -g root -m 0755 \
    "${ROOTFS_DIR}/etc/cardputerzero/trust/store" \
    "${ROOTFS_DIR}/etc/cardputerzero/trust/developers" \
    "${ROOTFS_DIR}/etc/cardputerzero/trust/revoked"
for store_key in "${payload}"/trust/store/*.pub; do
    if [ -f "$store_key" ] && [ ! -L "$store_key" ]; then
        install -o root -g root -m 0644 "$store_key" \
            "${ROOTFS_DIR}/etc/cardputerzero/trust/store/$(basename "$store_key")"
    fi
done
install -D -o root -g root -m 0644 "${payload}/lora.conf" \
    "${ROOTFS_DIR}/etc/cardputerzero/lora.conf"
install -D -o root -g root -m 0644 "${payload}/store.conf" \
    "${ROOTFS_DIR}/etc/cardputerzero/store.conf"
if [[ $access_profile == production ]]; then
    device_policy="${payload}/device-policy-production.json"
else
    device_policy="${payload}/device-policy.json"
fi
install -D -o root -g root -m 0644 "$device_policy" \
    "${ROOTFS_DIR}/etc/cardputerzero/device-policy.json"
install -D -m 0755 "${payload}/diagnostics/device-core-recovery.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-core-recovery"
install -D -m 0755 "${payload}/diagnostics/device-capability-acceptance.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-capability-acceptance"
install -D -m 0755 "${payload}/diagnostics/device-factory-acceptance.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-factory-acceptance"
install -D -m 0755 "${payload}/diagnostics/device-performance-acceptance.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-performance-acceptance"
install -D -m 0755 "${payload}/diagnostics/device-recovery-data.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-recovery-data"
install -D -m 0755 "${payload}/diagnostics/device-stability-monitor.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-stability-monitor"
install -D -m 0755 "${payload}/diagnostics/device-store-acceptance.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-store-acceptance"
install -D -m 0755 "${payload}/diagnostics/device-support-bundle.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-support-bundle"
install -D -m 0644 "${payload}/hello/app.json" "${hello_root}/app.json"
install -D -m 0644 "${payload}/hello/bin/hello-card.wasm" \
    "${hello_root}/bin/hello-card.wasm"

on_chroot <<'CHROOT'
set -e

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
if ! getent group cp0-provision-control >/dev/null 2>&1; then
    groupadd --system cp0-provision-control
fi
usermod -a -G cp0-provision-control cp0-shell
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

install -d -o root -g root -m 0755 \
    /var/lib/cardputerzero \
    /var/lib/cardputerzero/apps \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0 \
    /var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin
install -d -o root -g root -m 0700 /var/lib/cardputerzero/registry
install -d -o cp0-storage -g cp0-storage -m 0700 \
    /var/lib/cardputerzero/data
install -d -o cp0-document -g cp0-document -m 0750 \
    /var/lib/cardputerzero/documents
printf '%s\n' 'CardputerZero Document Portal is ready.' \
    >/var/lib/cardputerzero/documents/welcome.txt
chown cp0-document:cp0-document \
    /var/lib/cardputerzero/documents/welcome.txt
chmod 0640 /var/lib/cardputerzero/documents/welcome.txt
chown -R root:root /var/lib/cardputerzero/apps/dev.cardputerzero.hello
chmod -R go-w /var/lib/cardputerzero/apps/dev.cardputerzero.hello
/usr/libexec/cardputerzero/cp0-appd register-installed \
    dev.cardputerzero.hello 0.1.0

systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-appd.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-display.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-connectivity.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-gpio.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-storage.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-trust.conf
systemd-tmpfiles --create /usr/lib/tmpfiles.d/cardputerzero-store.conf
CHROOT

if [[ $image_profile == product ]]; then
    factory_root="${ROOTFS_DIR}/tmp/cardputerzero-factory-data"
    factory_bundle="${ROOTFS_DIR}/usr/share/cardputerzero/factory-data-v1.cp0backup"
    rm -rf "$factory_root"
    rm -f "$factory_bundle"
    install -d -o root -g root -m 0700 \
        "$factory_root" \
        "$factory_root/cardputerzero" \
        "$factory_root/etc-cardputerzero" \
        "$factory_root/extrausers" \
        "$factory_root/home" \
        "$factory_root/network-connections" \
        "$factory_root/network-state" \
        "$factory_root/ssh"
    cp -a "${ROOTFS_DIR}/var/lib/cardputerzero/." \
        "$factory_root/cardputerzero/"
    cp -a "${ROOTFS_DIR}/etc/cardputerzero/." \
        "$factory_root/etc-cardputerzero/"
    for database in passwd group shadow gshadow; do
        : >"$factory_root/extrausers/$database"
    done
    chmod 0644 "$factory_root/extrausers/passwd" "$factory_root/extrausers/group"
    chmod 0600 "$factory_root/extrausers/shadow" "$factory_root/extrausers/gshadow"
    printf '%s\n' cp0-data-layout-v2 >"$factory_root/layout-version"
    printf '%s\n' product >"$factory_root/etc-cardputerzero/image-profile"
    : >"$factory_root/machine-id"
    : >"$factory_root/random-seed"
    chmod 0644 "$factory_root/layout-version" "$factory_root/machine-id"
    chmod 0600 "$factory_root/random-seed"
    install -d -o root -g root -m 0755 \
        "${ROOTFS_DIR}/usr/share/cardputerzero"
    on_chroot <<'CHROOT'
set -e
/usr/bin/cp0-recovery backup \
    /tmp/cardputerzero-factory-data \
    /usr/share/cardputerzero/factory-data-v1.cp0backup
CHROOT
    rm -rf "$factory_root"

    on_chroot <<'CHROOT'
set -e
systemctl enable cardputerzero-appd.socket cardputerzero-broker.socket \
    cardputerzero-networkd.socket cardputerzero-documentd.socket \
    cardputerzero-audiod.socket cardputerzero-camerad.socket \
    cardputerzero-connectivityd.socket cardputerzero-displayd.socket \
    cardputerzero-provisiond.socket cardputerzero-provision-apply.service \
    cardputerzero-gpiod.socket cardputerzero-radiod.socket \
    cardputerzero-storaged.socket cardputerzero-stored.socket
CHROOT
else
    on_chroot <<'CHROOT'
set -e
systemctl mask cardputerzero-appd.service \
    cardputerzero-appd.socket cardputerzero-broker.socket \
    cardputerzero-networkd.socket cardputerzero-documentd.socket \
    cardputerzero-audiod.socket cardputerzero-camerad.socket \
    cardputerzero-connectivityd.service cardputerzero-connectivityd.socket \
    cardputerzero-provisiond.service cardputerzero-provisiond.socket \
    cardputerzero-provision-apply.service \
    cardputerzero-displayd.service cardputerzero-displayd.socket \
    cardputerzero-gpiod.socket cardputerzero-radiod.socket \
    cardputerzero-storaged.socket cardputerzero-stored.socket
CHROOT
fi
