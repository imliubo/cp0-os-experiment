#!/bin/bash -e

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
if [[ $image_profile == recovery && $access_profile != development ]]; then
    echo "error: recovery image requires development access" >&2
    exit 1
fi

BSP_REPOSITORY="https://github.com/m5stack/m5stack-linux-dtoverlays.git"
BSP_COMMIT="c3b254819307c177a34100b66fe19e52059ce8c4"
LEGACY_BOOTSCREEN_FIRMWARE_SHA256="d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd"
EARLY_SPLASH_SHA256="75a53d81f5ec087536a030919698c595630d48296e07d5f5f3d04ebebf2efd57"

install -m 0644 "${STAGE_DIR}/00-bsp/files/0001-tca8418-flush-synthetic-shift.patch" \
    "${ROOTFS_DIR}/tmp/0001-tca8418-flush-synthetic-shift.patch"
install -m 0644 "${STAGE_DIR}/00-bsp/files/0002-cardputerzero-v06-backlight-zero-duty.patch" \
    "${ROOTFS_DIR}/tmp/0002-cardputerzero-v06-backlight-zero-duty.patch"
install -m 0644 "${STAGE_DIR}/00-bsp/files/early-splash-spi.c" \
    "${ROOTFS_DIR}/tmp/cardputerzero-early-splash-spi.c"

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
git -C /tmp/cardputerzero-bsp apply --unidiff-zero \
    /tmp/0001-tca8418-flush-synthetic-shift.patch
git -C /tmp/cardputerzero-bsp apply --check \
    /tmp/0002-cardputerzero-v06-backlight-zero-duty.patch
git -C /tmp/cardputerzero-bsp apply \
    /tmp/0002-cardputerzero-v06-backlight-zero-duty.patch
rm -f /tmp/0001-tca8418-flush-synthetic-shift.patch \
    /tmp/0002-cardputerzero-v06-backlight-zero-duty.patch

# M5Stack validated 20 MHz on its display-stability branch. Keep the newer
# keyboard fixes from the pinned mainline BSP while applying that narrow LCD
# electrical limit here.
panel_overlay=/tmp/cardputerzero-bsp/modules/CardputerZero/cardputerzero-v5-overlay.dts
if grep -Fq 'power-supply = <&backlight_power>;' "\$panel_overlay"; then
    echo "error: V0.6 backlight must keep zero-duty PWM actively driven" >&2
    exit 1
fi
test "\$(grep -Fc 'spi-max-frequency = <60000000>;' "\$panel_overlay")" = 1
sed -i 's/spi-max-frequency = <60000000>;/spi-max-frequency = <20000000>;/' \
    "\$panel_overlay"
grep -Fq 'spi-max-frequency = <20000000>;' "\$panel_overlay"

KVER=\$(find /lib/modules -mindepth 1 -maxdepth 1 -type d -name '*rpi-v8*' \
    -printf '%f\n' | sort -V | tail -1)
test -n "\$KVER"

make -C /tmp/cardputerzero-bsp/modules/CardputerZero \
    CONFIG_CARDPUTERO_V0_5=y \
    KERNELDIR="/lib/modules/\$KVER/build" \
    EXTRADIR="/lib/modules/\$KVER/extra" \
    install
depmod -a "\$KVER"

mkdir -p /usr/libexec/cardputerzero
gcc -std=c11 -static -Os -Wall -Wextra -Werror \
    -fno-ident -fno-unwind-tables -fno-asynchronous-unwind-tables \
    /tmp/cardputerzero-early-splash-spi.c \
    -o /usr/libexec/cardputerzero/early-splash-spi
strip /usr/libexec/cardputerzero/early-splash-spi

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
rm -f /tmp/cardputerzero-early-splash-spi.c
apt-get purge -y build-essential device-tree-compiler git \
    linux-headers-rpi-v8 linux-headers-rpi-2712
apt-get autoremove -y --purge
apt-get clean
CHROOT

if [[ $image_profile == product ]]; then
    on_chroot <<'CHROOT'
set -e
systemctl mask systemd-machine-id-commit.service
CHROOT
fi

boot_config="${ROOTFS_DIR}/boot/firmware/config.txt"
cmdline="${ROOTFS_DIR}/boot/firmware/cmdline.txt"

sed -i -E '/^dtoverlay=vc4-kms-v3d(,cma-[0-9]+)?[[:space:]]*$/d' "$boot_config"
sed -i -E '/^camera_auto_detect=/d' "$boot_config"
sed -i -E '/^start_x=/d' "$boot_config"
sed -i '/^gpu_mem=/d' "$boot_config"
sed -i '/^gpu_mem_512=/d' "$boot_config"
sed -i '/^# CardputerZero OS CM0 V0.6 BSP$/d' "$boot_config"
sed -i '/^# BEGIN CardputerZero OS BSP$/,/^# END CardputerZero OS BSP$/d' \
    "$boot_config"
for managed_line in \
    'enable_uart=1' \
    'dtoverlay=dwc2' \
    'dtoverlay=dwc2,dr_mode=peripheral' \
    'dtoverlay=imx219' \
    'dtoverlay=camera-py12-high-overlay' \
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
camera_auto_detect=0
start_x=1
enable_uart=1
dtoverlay=dwc2,dr_mode=peripheral
dtoverlay=cardputerzero-v5-overlay
dtoverlay=imx219
dtoverlay=bq27220_v5
dtoverlay=bmi270_bmm150_overlay
dtoverlay=gpio-ir,gpio_pin=13,gpio_pull=up
dtoverlay=gpio-ir-tx,gpio_pin=12
# END CardputerZero OS BSP
CONFIG

for camera_firmware in start_x.elf fixup_x.dat; do
    if [[ ! -s ${ROOTFS_DIR}/boot/firmware/$camera_firmware ]]; then
        echo "error: raspi-firmware is missing $camera_firmware" >&2
        exit 1
    fi
done
for firmware in start.elf start_x.elf; do
    firmware_sha256=$(sha256sum "${ROOTFS_DIR}/boot/firmware/$firmware" |
        awk '{print $1}')
    if [[ $firmware_sha256 == "$LEGACY_BOOTSCREEN_FIRMWARE_SHA256" ]]; then
        echo "error: legacy M5Stack firmware forces an invalid 256/256 memory split" >&2
        exit 1
    fi
done

early_splash="${STAGE_DIR}/00-bsp/files/splash.rgb565"
printf '%s  %s\n' "$EARLY_SPLASH_SHA256" "$early_splash" | sha256sum -c -
install -D -m 0644 "$early_splash" \
    "${ROOTFS_DIR}/usr/share/cardputerzero/boot/splash.rgb565"
on_chroot <<'CHROOT'
set -e
/usr/libexec/cardputerzero/early-splash-spi --check-image \
    /usr/share/cardputerzero/boot/splash.rgb565
CHROOT

for pattern in \
    quiet splash 'logo\.nologo' 'loglevel=[^[:space:]]+' \
    'consoleblank=[^[:space:]]+' 'fbcon=map:[^[:space:]]+' \
    'vt\.global_cursor_default=[^[:space:]]+' \
    'systemd\.show_status=[^[:space:]]+' \
    'rd\.systemd\.show_status=[^[:space:]]+'; do
    sed -i -E "s/(^|[[:space:]])${pattern}([[:space:]]|$)/ /g" "$cmdline"
done
sed -i -E 's/(^|[[:space:]])resize([[:space:]]|$)/ /g' "$cmdline"
sed -i -E \
    's/(^|[[:space:]])cp0\.overlay_root=volatile([[:space:]]|$)/ /g' \
    "$cmdline"
sed -i -E 's/[[:space:]]+/ /g; s/^ //; s/ $//' "$cmdline"

for token in cgroup_memory=1 cgroup_enable=memory apparmor=1 security=apparmor; do
    if ! grep -qw "$token" "$cmdline"; then
        sed -i "s|$| $token|" "$cmdline"
    fi
done
if [[ $image_profile == product ]]; then
    boot_tokens='quiet loglevel=3 logo.nologo vt.global_cursor_default=0 consoleblank=0 fbcon=map:off systemd.show_status=false rd.systemd.show_status=false'
    sed -i 's|$| cp0.overlay_root=volatile|' "$cmdline"
else
    boot_tokens='loglevel=6 consoleblank=0 fbcon=map:1'
fi
for token in $boot_tokens; do
    if ! grep -qw "$token" "$cmdline"; then
        sed -i "s|$| $token|" "$cmdline"
    fi
done

install -d -o root -g root -m 0755 \
    "${ROOTFS_DIR}/etc/cardputerzero"
printf '%s\n' "$image_profile" \
    >"${ROOTFS_DIR}/etc/cardputerzero/image-profile"
chmod 0644 "${ROOTFS_DIR}/etc/cardputerzero/image-profile"
printf '%s\n' "$access_profile" \
    >"${ROOTFS_DIR}/etc/cardputerzero/access-profile"
chmod 0644 "${ROOTFS_DIR}/etc/cardputerzero/access-profile"

install -D -m 0755 "${STAGE_DIR}/00-bsp/files/device-smoke.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/device-smoke.sh"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/camera-probe.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/camera-probe"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/cardputerzero-camera-probe.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-camera-probe.service"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/show-early-splash.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/show-early-splash.sh"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/cardputerzero-early-splash.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-early-splash.service"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/console-banner.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/console-banner.sh"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/cardputerzero-console-banner.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-console-banner.service"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/prepare-ssh.sh" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/prepare-ssh.sh"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/cardputerzero-ssh-prepare.service" \
    "${ROOTFS_DIR}/usr/lib/systemd/system/cardputerzero-ssh-prepare.service"
install -D -m 0644 "${STAGE_DIR}/00-bsp/files/avahi-daemon.conf" \
    "${ROOTFS_DIR}/etc/avahi/avahi-daemon.conf"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/cardputerzero-firmware.initramfs-hook" \
    "${ROOTFS_DIR}/etc/initramfs-tools/hooks/cardputerzero-firmware"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/overlay-root-initramfs" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/overlay-root-initramfs"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/data-grow-initramfs" \
    "${ROOTFS_DIR}/usr/libexec/cardputerzero/data-grow-initramfs"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/overlay-root-initramfs" \
    "${ROOTFS_DIR}/etc/initramfs-tools/scripts/init-bottom/cardputerzero-overlay-root"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/data-grow-initramfs" \
    "${ROOTFS_DIR}/etc/initramfs-tools/scripts/local-premount/cardputerzero-data-grow"
install -D -m 0755 "${STAGE_DIR}/00-bsp/files/cardputerzero-overlay-root.initramfs-hook" \
    "${ROOTFS_DIR}/etc/initramfs-tools/hooks/cardputerzero-overlay-root"
if [[ $image_profile == product ]]; then
    install -D -m 0755 "${STAGE_DIR}/00-bsp/files/early-splash-initramfs" \
        "${ROOTFS_DIR}/etc/initramfs-tools/scripts/init-top/cardputerzero-early-splash"
fi
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
    bluez bluez-firmware modemmanager udisks2 \
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
if id -u "$FIRST_USER_NAME" >/dev/null 2>&1; then
    for group in adm dialout audio users sudo video input gpio spi i2c netdev render; do
        adduser "$FIRST_USER_NAME" "$group"
    done
    if [ -n "${PUBKEY_SSH_FIRST_USER:-}" ]; then
        chown -R "$FIRST_USER_NAME:$FIRST_USER_NAME" \
            "/home/$FIRST_USER_NAME/.ssh"
    fi
fi
usermod --lock root

sed -i -E \
    -e 's/^#[[:space:]]*(en_US.UTF-8[[:space:]]+UTF-8)$/\1/' \
    -e 's/^#[[:space:]]*(zh_CN.UTF-8[[:space:]]+UTF-8)$/\1/' \
    /etc/locale.gen
locale-gen en_US.UTF-8 zh_CN.UTF-8
locale -a | grep -qx 'en_US.utf8'
locale -a | grep -qx 'zh_CN.utf8'

printf '%s\n' "$TIMEZONE_DEFAULT" >/etc/timezone
ln -sf "/usr/share/zoneinfo/$TIMEZONE_DEFAULT" /etc/localtime
if [ -n "${WPA_COUNTRY:-}" ]; then
    raspi_config_user=$FIRST_USER_NAME
    if ! id -u "$raspi_config_user" >/dev/null 2>&1; then
        raspi_config_user=root
    fi
    SUDO_USER="$raspi_config_user" raspi-config nonint do_wifi_country "$WPA_COUNTRY"
fi

systemctl disable \
    apt-daily.timer \
    apt-daily-upgrade.timer \
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
systemctl enable NetworkManager.service apparmor.service \
    avahi-daemon.service \
    cardputerzero-camera-probe.service \
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
tmpfs /var/tmp tmpfs nodev,nosuid,noatime,mode=1777,size=128M 0 0
# END CardputerZero volatile filesystems
FSTAB
fi
rm -f /var/lib/systemd/random-seed
rm -f /etc/ssh/ssh_host_*_key*
apt-get clean
CHROOT

if [[ $image_profile == recovery ]]; then
    on_chroot <<'CHROOT'
set -e
systemctl enable cardputerzero-console-banner.service
CHROOT
else
    on_chroot <<'CHROOT'
set -e
systemctl disable cardputerzero-console-banner.service 2>/dev/null || true
systemctl enable cardputerzero-early-splash.service
CHROOT
fi

if [[ $access_profile == development ]]; then
    on_chroot <<'CHROOT'
set -e
systemctl enable ssh.service cardputerzero-ssh-prepare.service
CHROOT
else
    on_chroot <<'CHROOT'
set -e
test "$FIRST_USER_NAME" = cp0-build
build_uid=1000
rm -f "/etc/sudoers.d/010_${FIRST_USER_NAME}-nopasswd" \
    /etc/sudoers.d/010_pi-nopasswd
if getent passwd "$FIRST_USER_NAME" >/dev/null 2>&1; then
    test "$(id -u "$FIRST_USER_NAME")" = "$build_uid"
    userdel --remove "$FIRST_USER_NAME"
fi
if getent passwd "$FIRST_USER_NAME" >/dev/null 2>&1 ||
   find / -xdev -uid "$build_uid" -print -quit 2>/dev/null | grep -q .; then
    echo "error: temporary product build identity remains" >&2
    exit 1
fi
systemctl disable ssh.service ssh.socket ssh@.service sshd.service \
    sshd@.service cardputerzero-ssh-prepare.service \
    regenerate_ssh_host_keys.service getty@tty1.service \
    serial-getty@serial0.service 2>/dev/null || true
systemctl mask --force \
    regenerate_ssh_host_keys.service getty@.service getty@tty1.service \
    serial-getty@.service serial-getty@serial0.service \
    cardputerzero-recovery-console.service
CHROOT
fi

if [[ $image_profile == product ]]; then
    on_chroot <<'CHROOT'
set -e
for database in passwd group shadow; do
    if ! grep -Eq "^${database}:.*(^|[[:space:]])extrausers([[:space:]]|$)" "/etc/nsswitch.conf"; then
        sed -i -E "s/^(${database}:[^#]*)$/\\1 extrausers/" /etc/nsswitch.conf
    fi
    grep -Eq "^${database}:.*extrausers" /etc/nsswitch.conf
done
test -e /usr/lib/libnss_extrausers.so.2
CHROOT
fi
