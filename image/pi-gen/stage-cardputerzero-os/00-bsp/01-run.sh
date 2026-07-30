#!/bin/bash -e

BSP_REPOSITORY="https://github.com/m5stack/m5stack-linux-dtoverlays.git"
BSP_COMMIT="c3b254819307c177a34100b66fe19e52059ce8c4"

on_chroot <<CHROOT
set -e
apt-get update
apt-get install -y --no-install-recommends \
    build-essential \
    device-tree-compiler \
    git \
    linux-headers-rpi-v8

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
case " \$BOOTARGS " in
    *" cgroup_disable=memory "*) ;;
    *) echo "expected cgroup_disable=memory in \$CM0_DTB" >&2; exit 1 ;;
esac
FILTERED=
for token in \$BOOTARGS; do
    if [ "\$token" != cgroup_disable=memory ]; then
        FILTERED="\${FILTERED:+\$FILTERED }\$token"
    fi
done
fdtput -t s "\$CM0_DTB" /chosen bootargs "\$FILTERED"
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

cat >>"$boot_config" <<'CONFIG'

# CardputerZero OS CM0 V0.6 BSP
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
CONFIG

for token in quiet splash fbcon=map:off; do
    sed -i -E "s/(^|[[:space:]])${token}([[:space:]]|$)/ /g" "$cmdline"
done
sed -i -E 's/[[:space:]]+/ /g; s/^ //; s/ $//' "$cmdline"

for token in cgroup_memory=1 cgroup_enable=memory apparmor=1 security=apparmor loglevel=6 consoleblank=0 fbcon=map:0; do
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
    rpi-zram-writeback.timer 2>/dev/null || true
rm -f /etc/systemd/system/fb_load.service
systemctl mask apt-daily.service apt-daily.timer \
    apt-daily-upgrade.service apt-daily-upgrade.timer \
    fb_load.service 2>/dev/null || true
systemctl enable NetworkManager.service ssh.service apparmor.service \
    getty@tty1.service cardputerzero-console-banner.service
systemctl enable rpi-resize.service 2>/dev/null || true
systemctl set-default multi-user.target
for module in \
    gpio-forwarder panel-mipi-dbi-m pwm_bl_m5stack st7789v_m5stack \
    tca8418_keypad_m5stack; do
    if ! grep -qx "$module" /etc/initramfs-tools/modules; then
        printf '%s\n' "$module" >>/etc/initramfs-tools/modules
    fi
done
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
SYSCTL
rm -f /var/lib/systemd/random-seed
rm -f /etc/ssh/ssh_host_*_key*
apt-get clean
CHROOT
