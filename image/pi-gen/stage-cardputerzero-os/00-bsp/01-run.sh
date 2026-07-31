#!/bin/bash -e

BSP_REPOSITORY="https://github.com/m5stack/m5stack-linux-dtoverlays.git"
BSP_COMMIT="c3b254819307c177a34100b66fe19e52059ce8c4"

on_chroot <<CHROOT
set -e
for source in /etc/apt/sources.list /etc/apt/sources.list.d/*; do
    [ -f "\$source" ] || continue
    sed -i \
        -e 's|http://deb.debian.org|https://deb.debian.org|g' \
        -e 's|http://archive.raspberrypi.com|https://archive.raspberrypi.com|g' \
        "\$source"
done
apt-get update
apt-get install -y --no-install-recommends \
    build-essential \
    device-tree-compiler \
    git \
    linux-headers-rpi-v8

rm -rf /tmp/cardputerzero-bsp
http_proxy="$APT_PROXY" https_proxy="$APT_PROXY" \
    git clone --no-checkout "${BSP_REPOSITORY}" /tmp/cardputerzero-bsp
git -C /tmp/cardputerzero-bsp checkout "${BSP_COMMIT}"
test "\$(git -C /tmp/cardputerzero-bsp rev-parse HEAD)" = "${BSP_COMMIT}"

KVER=\$(find /lib/modules -mindepth 1 -maxdepth 1 -type d -name '*rpi-v8*' \
    -printf '%f\n' | sort -V | tail -1)
test -n "\$KVER"

make -C /tmp/cardputerzero-bsp/modules/CardputerZero \
    CONFIG_CARDPUTERO_V0_5=y \
    KERNELDIR="/lib/modules/\$KVER/build" \
    EXTRADIR="/lib/modules/\$KVER/extra" \
    install
depmod -a "\$KVER"

CM0_DTB=/boot/firmware/bcm2710-rpi-cm0.dtb
BOOTARGS=\$(fdtget -t s "\$CM0_DTB" /chosen bootargs)
FILTERED=
for token in \$BOOTARGS; do
    if [ "\$token" != cgroup_disable=memory ]; then
        FILTERED="\${FILTERED:+\$FILTERED }\$token"
    fi
done
if [ "\$BOOTARGS" != "\$FILTERED" ]; then
    fdtput -t s "\$CM0_DTB" /chosen bootargs "\$FILTERED"
fi
fdtget -t s "\$CM0_DTB" /chosen bootargs | grep -qv cgroup_disable=memory

rm -rf /tmp/cardputerzero-bsp
apt-get purge -y build-essential device-tree-compiler git \
    linux-headers-rpi-v8 linux-headers-rpi-2712
apt-get autoremove -y --purge
apt-get clean
CHROOT

boot_config="${ROOTFS_DIR}/boot/firmware/config.txt"
cmdline="${ROOTFS_DIR}/boot/firmware/cmdline.txt"

sed -i -E '/^dtoverlay=vc4-kms-v3d(,cma-[0-9]+)?[[:space:]]*$/d' "$boot_config"
sed -i '/^gpu_mem=/d' "$boot_config"
sed -i '/^gpu_mem_512=/d' "$boot_config"
sed -i '/^# CardputerZero OS CM0 V0.6 BSP$/d' "$boot_config"
sed -i '/^# BEGIN CardputerZero OS BSP$/,/^# END CardputerZero OS BSP$/d' \
    "$boot_config"
for managed_line in \
    'enable_uart=1' \
    'dtoverlay=dwc2' \
    'dtoverlay=cardputerzero-v5-overlay' \
    'dtoverlay=bq27220_v5' \
    'dtoverlay=bmi270_bmm150_overlay' \
    'dtoverlay=gpio-ir,gpio_pin=13,gpio_pull=up' \
    'dtoverlay=gpio-ir-tx,gpio_pin=12'; do
    sed -i "\|^${managed_line}$|d" "$boot_config"
done

cat >>"$boot_config" <<'CONFIG'

# BEGIN CardputerZero OS BSP
[all]
gpu_mem=64
gpu_mem_512=64
dtoverlay=vc4-kms-v3d,cma-64
enable_uart=1
dtoverlay=dwc2
dtoverlay=cardputerzero-v5-overlay
dtoverlay=bq27220_v5
dtoverlay=bmi270_bmm150_overlay
dtoverlay=gpio-ir,gpio_pin=13,gpio_pull=up
dtoverlay=gpio-ir-tx,gpio_pin=12
# END CardputerZero OS BSP
CONFIG

for token in quiet splash fbcon=map:off fbcon=map:0; do
    sed -i -E "s/(^|[[:space:]])${token}([[:space:]]|$)/ /g" "$cmdline"
done
sed -i -E 's/(^|[[:space:]])resize([[:space:]]|$)/ /g' "$cmdline"
sed -i -E 's/[[:space:]]+/ /g; s/^ //; s/ $//' "$cmdline"

for token in cgroup_memory=1 cgroup_enable=memory apparmor=1 security=apparmor loglevel=6 consoleblank=0 fbcon=map:1 cp0.overlay_root=volatile; do
    if ! grep -qw "$token" "$cmdline"; then
        sed -i "s|$| $token|" "$cmdline"
    fi
done

install -D -m 0755 "${STAGE_DIR}/00-bsp/files/device-smoke.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-smoke.sh"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/console-banner.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/console-banner.sh"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/cardputerzero-console-banner.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-console-banner.service"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/cardputerzero-firmware.initramfs-hook" \
    "${ROOTFS_DIR}/etc/initramfs-tools/hooks/cardputerzero-firmware"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/overlay-root-initramfs" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/overlay-root-initramfs"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/data-grow-initramfs" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/data-grow-initramfs"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/cardputerzero-overlay-root.initramfs-hook" \
    "${ROOTFS_DIR}/etc/initramfs-tools/hooks/cardputerzero-overlay-root"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/overlay-root-status.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/overlay-root-status"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/cardputerzero-overlay-root-status.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-overlay-root-status.service"

if [[ -n ${PUBKEY_SSH_FIRST_USER:-} ]]; then
    install -d -m 0700 \
        "${ROOTFS_DIR}/home/${FIRST_USER_NAME}/.ssh"
    printf '%s\n' "$PUBKEY_SSH_FIRST_USER" \
        >"${ROOTFS_DIR}/home/${FIRST_USER_NAME}/.ssh/authorized_keys"
    chmod 0600 "${ROOTFS_DIR}/home/${FIRST_USER_NAME}/.ssh/authorized_keys"
fi

mkdir -p "${ROOTFS_DIR}/etc/rpi/swap.conf.d"
cat >"${ROOTFS_DIR}/etc/rpi/swap.conf.d/90-cardputerzero-os.conf" <<'ZRAM'
[Main]
Mechanism=zram

[Zram]
FixedSizeMiB=192
ZRAM

on_chroot <<'CHROOT'
set -e
purge_packages=
for package in \
    cloud-init rpi-cloud-init-mods \
    binutils binutils-aarch64-linux-gnu binutils-common \
    cpp cpp-14 cpp-14-aarch64-linux-gnu cpp-aarch64-linux-gnu \
    gcc gcc-14 gcc-14-aarch64-linux-gnu gcc-aarch64-linux-gnu \
    dpkg-dev libdpkg-perl libc-dev-bin libc6-dev linux-libc-dev make \
    lightdm wayfire wf-panel-pi pcmanfm pcmanfm-qt \
    packagekit pipewire pipewire-pulse wireplumber libinput-tools \
    avahi-daemon bluez bluez-firmware modemmanager udisks2 \
    rpi-connect rpi-connect-lite rpi-connect-wayvnc \
    rpicam-apps-lite mkvtoolnix cifs-utils ntfs-3g libmtp-runtime \
    wolfram-engine gdb htop man-db manpages-dev ncdu strace; do
    if dpkg-query -W -f='${db:Status-Abbrev}' "$package" 2>/dev/null | grep -q '^ii '; then
        purge_packages="$purge_packages $package"
    fi
done
for package in $(dpkg-query -W -f='${binary:Package}\n' \
    'linux-base-*rpi-2712' 'linux-image-*rpi-2712' \
    'linux-headers-*' 'linux-kbuild-*' 2>/dev/null || true); do
    purge_packages="$purge_packages $package"
done
if [ -n "$purge_packages" ]; then
    apt-get purge -y $purge_packages
fi
apt-get autoremove -y --purge

for group in input spi i2c gpio; do
    groupadd -f -r "$group"
done
for group in adm dialout audio users sudo video input gpio spi i2c netdev render; do
    adduser "$FIRST_USER_NAME" "$group"
done
if [ -n "${PUBKEY_SSH_FIRST_USER:-}" ]; then
    chown -R "$FIRST_USER_NAME:$FIRST_USER_NAME" \
        "/home/$FIRST_USER_NAME/.ssh"
fi
usermod --lock root

printf '%s\n' "$TIMEZONE_DEFAULT" >/etc/timezone
ln -sf "/usr/share/zoneinfo/$TIMEZONE_DEFAULT" /etc/localtime
if [ -n "${WPA_COUNTRY:-}" ]; then
    SUDO_USER="$FIRST_USER_NAME" raspi-config nonint do_wifi_country "$WPA_COUNTRY"
fi

systemctl disable \
    apt-daily.timer \
    apt-daily-upgrade.timer \
    avahi-daemon.service \
    bluetooth.service \
    ModemManager.service \
    nfs-blkmap.service \
    rpcbind.service \
    udisks2.service \
    rpi-connect.service \
    rpi-connect-wayvnc.service \
    fb_load.service \
    rpi-zram-writeback.service \
    rpi-zram-writeback.timer \
    rpi-resize.service 2>/dev/null || true
rm -f /etc/systemd/system/fb_load.service
systemctl mask apt-daily.service apt-daily.timer \
    apt-daily-upgrade.service apt-daily-upgrade.timer \
    fb_load.service 2>/dev/null || true
systemctl enable NetworkManager.service ssh.service apparmor.service \
    getty@tty1.service cardputerzero-console-banner.service \
    cardputerzero-overlay-root-status.service
systemctl set-default multi-user.target
for module in \
    overlay gpio-forwarder panel-mipi-dbi-m pwm_bl_m5stack st7789v_m5stack \
    tca8418_keypad_m5stack; do
    if ! grep -qx "$module" /etc/initramfs-tools/modules; then
        printf '%s\n' "$module" >>/etc/initramfs-tools/modules
    fi
done
update-initramfs -u -k all
mkdir -p /etc/systemd/journald.conf.d
cat >/etc/systemd/journald.conf.d/10-cardputerzero-os.conf <<'JOURNALD'
[Journal]
Storage=volatile
RuntimeMaxUse=16M
JOURNALD
cat >/etc/sysctl.d/90-cardputerzero-os.conf <<'SYSCTL'
vm.swappiness=100
vm.dirty_background_ratio=5
vm.dirty_ratio=10
fs.protected_fifos=2
fs.protected_hardlinks=1
fs.protected_regular=2
fs.protected_symlinks=1
fs.suid_dumpable=0
kernel.core_pattern=/dev/null
kernel.dmesg_restrict=1
kernel.kptr_restrict=2
kernel.unprivileged_bpf_disabled=1
SYSCTL
if ! grep -q '^# BEGIN CardputerZero volatile filesystems$' /etc/fstab; then
    cat >>/etc/fstab <<'FSTAB'

# BEGIN CardputerZero volatile filesystems
tmpfs /tmp tmpfs nodev,nosuid,noatime,mode=1777,size=32M 0 0
tmpfs /var/tmp tmpfs nodev,nosuid,noatime,mode=1777,size=8M 0 0
# END CardputerZero volatile filesystems
FSTAB
fi
rm -f /var/lib/systemd/random-seed
rm -f /etc/ssh/ssh_host_*_key*
apt-get clean
CHROOT
