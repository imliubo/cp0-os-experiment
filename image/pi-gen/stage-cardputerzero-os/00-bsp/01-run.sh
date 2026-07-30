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
apt-get purge -y build-essential device-tree-compiler git linux-headers-rpi-v8
apt-get autoremove -y --purge
apt-get clean
CHROOT

boot_config="${ROOTFS_DIR}/boot/firmware/config.txt"
cmdline="${ROOTFS_DIR}/boot/firmware/cmdline.txt"

sed -i -E \
    's/^dtoverlay=vc4-kms-v3d(,cma-[0-9]+)?[[:space:]]*$/dtoverlay=vc4-kms-v3d,cma-64/' \
    "$boot_config"
if ! grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$boot_config"; then
    sed -i '/^\[all\]$/i dtoverlay=vc4-kms-v3d,cma-64' "$boot_config"
fi

sed -i '/^gpu_mem=/d' "$boot_config"
sed -i '/^gpu_mem_512=/d' "$boot_config"
sed -i '/^\[all\]$/i gpu_mem=64' "$boot_config"
sed -i '/^\[all\]$/i gpu_mem_512=64' "$boot_config"

cat >>"$boot_config" <<'CONFIG'

# CardputerZero OS CM0 V0.6 BSP
[all]
enable_uart=1
dtoverlay=dwc2
dtoverlay=cardputerzero-v5-overlay
dtoverlay=bq27220_v5
dtoverlay=bmi270_bmm150_overlay
dtoverlay=gpio-ir,gpio_pin=13,gpio_pull=up
dtoverlay=gpio-ir-tx,gpio_pin=12
CONFIG

for token in cgroup_memory=1 cgroup_enable=memory apparmor=1 security=apparmor quiet fbcon=map:off; do
    if ! grep -qw "$token" "$cmdline"; then
        sed -i "s|$| $token|" "$cmdline"
    fi
done

install -D -m 0755 "${STAGE_DIR}/00-bsp/files/device-smoke.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-smoke.sh"

mkdir -p "${ROOTFS_DIR}/etc/rpi/swap.conf.d"
cat >"${ROOTFS_DIR}/etc/rpi/swap.conf.d/90-cardputerzero-os.conf" <<'ZRAM'
[Main]
Mechanism=zram

[Zram]
FixedSizeMiB=192
ZRAM

on_chroot <<'CHROOT'
set -e
apt-get purge -y cloud-init rpi-cloud-init-mods || true
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
    rpi-zram-writeback.service \
    rpi-zram-writeback.timer 2>/dev/null || true
systemctl enable NetworkManager.service ssh.service apparmor.service
rm -f /var/lib/systemd/random-seed
apt-get clean
CHROOT
