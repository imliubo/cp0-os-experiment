#!/usr/bin/env bash
set -euo pipefail

rootfs=${1:-${ROOTFS_DIR:-}}
if [[ -z $rootfs || ! -d $rootfs ]]; then
    echo "usage: $0 ROOTFS_DIR" >&2
    exit 2
fi
rootfs=$(cd "$rootfs" && pwd -P)
bootfs="$rootfs/boot/firmware"
image_profile=$(cat "$rootfs/etc/cardputerzero/image-profile" 2>/dev/null || true)
case "$image_profile" in
    product | recovery) ;;
    *)
        echo "error: missing or invalid image profile: ${image_profile:-missing}" >&2
        exit 1
        ;;
esac
access_profile=$(cat "$rootfs/etc/cardputerzero/access-profile" 2>/dev/null || true)
case "$access_profile" in
    development | production) ;;
    *)
        echo "error: missing or invalid access profile: ${access_profile:-missing}" >&2
        exit 1
        ;;
esac
if [[ $image_profile == recovery && $access_profile != development ]]; then
    echo "error: recovery image has production access profile" >&2
    exit 1
fi

required_executables=(
    usr/bin/cardputerzero-system-shell
    usr/bin/cp0-recovery
    usr/bin/cp0ctl
    usr/libexec/cardputerzero/app-runtime
    usr/libexec/cardputerzero/cp0-appd
    usr/libexec/cardputerzero/cp0-audiod
    usr/libexec/cardputerzero/cp0-camerad
    usr/libexec/cardputerzero/cp0-documentd
    usr/libexec/cardputerzero/cp0-devd
    usr/libexec/cardputerzero/cp0-gpiod
    usr/libexec/cardputerzero/cp0-networkd
    usr/libexec/cardputerzero/cp0-powerd
    usr/libexec/cardputerzero/cp0-provisiond
    usr/libexec/cardputerzero/cp0-radiod
    usr/libexec/cardputerzero/cp0-storaged
    usr/libexec/cardputerzero/cp0-stored
    usr/libexec/cardputerzero/cp0-usb-mediad
    usr/libexec/cardputerzero/device-core-recovery
    usr/libexec/cardputerzero/device-capability-acceptance
    usr/libexec/cardputerzero/device-factory-acceptance
    usr/libexec/cardputerzero/device-performance-acceptance
    usr/libexec/cardputerzero/device-recovery-data
    usr/libexec/cardputerzero/device-smoke.sh
    usr/libexec/cardputerzero/camera-probe
    usr/libexec/cardputerzero/early-splash-spi
    usr/libexec/cardputerzero/show-early-splash.sh
    usr/libexec/cardputerzero/device-stability-monitor
    usr/libexec/cardputerzero/device-store-acceptance
    usr/libexec/cardputerzero/device-support-bundle
    usr/libexec/cardputerzero/map-recovery-console.sh
    usr/libexec/cardputerzero/data-grow-initramfs
    usr/libexec/cardputerzero/overlay-root-initramfs
    usr/libexec/cardputerzero/prepare-ssh.sh
    usr/lib/systemd/system-generators/cardputerzero-display-generator
    etc/initramfs-tools/scripts/init-bottom/cardputerzero-overlay-root
    etc/initramfs-tools/scripts/local-premount/cardputerzero-data-grow
)
for path in "${required_executables[@]}"; do
    if [[ ! -x $rootfs/$path || -L $rootfs/$path ]]; then
        echo "error: required executable missing or symbolic: /$path" >&2
        exit 1
    fi
done

required_files=(
    usr/lib/aarch64-linux-gnu/weston/cardputerzero-policy.so
    etc/cardputerzero/device-policy.json
    etc/avahi/avahi-daemon.conf
    usr/lib/systemd/system/cardputerzero-usb-mediad.service
    usr/lib/systemd/system/cardputerzero-usb-mediad.socket
    usr/lib/tmpfiles.d/cardputerzero-usb-media.conf
    usr/lib/modules-load.d/cardputerzero-usb-media.conf
    usr/lib/systemd/system/cardputerzero-early-splash.service
    usr/share/cardputerzero/boot/splash.rgb565
    usr/share/cardputerzero/builtin-apps.tsv
    var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.hello/0.1.0/bin/hello-card.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.calculator/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.calculator/0.1.0/bin/calculator.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.neon-snake/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.neon-snake/0.1.0/bin/neon_snake.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.camera/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.camera/0.1.0/bin/camera.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.gallery/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.gallery/0.1.0/bin/gallery.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.music/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.music/0.1.0/bin/music_player.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.notes/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.notes/0.1.0/bin/notes.wasm
    var/lib/cardputerzero/apps/dev.cardputerzero.stopwatch/0.1.0/app.json
    var/lib/cardputerzero/apps/dev.cardputerzero.stopwatch/0.1.0/bin/stopwatch.wasm
)
for path in "${required_files[@]}"; do
    if [[ ! -f $rootfs/$path || -L $rootfs/$path ]]; then
        echo "error: required image file missing or symbolic: /$path" >&2
        exit 1
    fi
done

expected_builtin_ids=(
    dev.cardputerzero.calculator
    dev.cardputerzero.camera
    dev.cardputerzero.gallery
    dev.cardputerzero.hello
    dev.cardputerzero.music
    dev.cardputerzero.neon-snake
    dev.cardputerzero.notes
    dev.cardputerzero.stopwatch
)
mapfile -t builtin_ids < <(
    awk -F '\t' 'NF && $1 !~ /^#/ {print $2}' \
        "$rootfs/usr/share/cardputerzero/builtin-apps.tsv" | sort -u
)
if [[ ${#builtin_ids[@]} -ne ${#expected_builtin_ids[@]} ]] ||
    [[ $(printf '%s\n' "${builtin_ids[@]}") != \
       $(printf '%s\n' "${expected_builtin_ids[@]}") ]]; then
    echo "error: image built-in application allowlist is not the fixed eight-app set" >&2
    exit 1
fi

keyboard_manifest="$rootfs/var/lib/cardputerzero/apps/dev.cardputerzero.keyboard-diagnostics/0.1.0/app.json"
keyboard_wasm="$rootfs/var/lib/cardputerzero/apps/dev.cardputerzero.keyboard-diagnostics/0.1.0/bin/keyboard_diagnostics.wasm"
if [[ -e $keyboard_manifest || -e $keyboard_wasm ]]; then
    for diagnostic_file in "$keyboard_manifest" "$keyboard_wasm"; do
        if [[ ! -f $diagnostic_file || -L $diagnostic_file ]]; then
            echo "error: optional keyboard diagnostics payload is incomplete" >&2
            exit 1
        fi
    done
    jq_filter='
        .id == "dev.cardputerzero.keyboard-diagnostics" and
        .version == "0.1.0" and
        .permissions == []
    '
    if command -v jq >/dev/null 2>&1; then
        jq -e "$jq_filter" "$keyboard_manifest" >/dev/null
    elif [[ $EUID -eq 0 && -x $rootfs/usr/bin/jq ]]; then
        chroot "$rootfs" /usr/bin/jq -e "$jq_filter" \
            /var/lib/cardputerzero/apps/dev.cardputerzero.keyboard-diagnostics/0.1.0/app.json \
            >/dev/null
    else
        echo "error: jq is required to validate keyboard diagnostics" >&2
        exit 1
    fi
fi
for marker in developer-mode recovery-mode; do
    if [[ -e $rootfs/var/lib/cardputerzero/registry/$marker ]]; then
        echo "error: image enables $marker by default" >&2
        exit 1
    fi
done

enabled_units=(
    multi-user.target.wants/cardputerzero-camera-probe.service
)
if [[ $image_profile == recovery ]]; then
    enabled_units+=(
        multi-user.target.wants/cardputerzero-console-banner.service
    )
fi
if [[ $access_profile == development ]]; then
    enabled_units+=(
        multi-user.target.wants/ssh.service
        ssh.service.requires/cardputerzero-ssh-prepare.service
        sysinit.target.wants/regenerate_ssh_host_keys.service
    )
fi
if [[ $image_profile == product ]]; then
    enabled_units+=(
        multi-user.target.wants/cardputerzero-early-splash.service
        multi-user.target.wants/cardputerzero-overlay-root-status.service
        multi-user.target.wants/avahi-daemon.service
        multi-user.target.wants/seatd.service
        sockets.target.wants/cardputerzero-appd.socket
        sockets.target.wants/cardputerzero-audiod.socket
        sockets.target.wants/cardputerzero-broker.socket
        sockets.target.wants/cardputerzero-camerad.socket
        sockets.target.wants/cardputerzero-documentd.socket
        sockets.target.wants/cardputerzero-devd.socket
        sockets.target.wants/cardputerzero-gpiod.socket
        sockets.target.wants/cardputerzero-networkd.socket
        sockets.target.wants/cardputerzero-powerd.socket
        sockets.target.wants/cardputerzero-provisiond.socket
        multi-user.target.wants/cardputerzero-provision-apply.service
        sockets.target.wants/cardputerzero-radiod.socket
        sockets.target.wants/cardputerzero-storaged.socket
        sockets.target.wants/cardputerzero-stored.socket
        sockets.target.wants/cardputerzero-usb-mediad.socket
        multi-user.target.wants/cardputerzero-ssh-access.path
    )
fi
for path in "${enabled_units[@]}"; do
    if [[ ! -L $rootfs/etc/systemd/system/$path ]]; then
        echo "error: required unit is not enabled: $path" >&2
        exit 1
    fi
done
banner_link="$rootfs/etc/systemd/system/multi-user.target.wants/cardputerzero-console-banner.service"
if [[ $image_profile == product && ( -e $banner_link || -L $banner_link ) ]]; then
    echo "error: product image enables the LCD console banner" >&2
    exit 1
fi
early_splash_link="$rootfs/etc/systemd/system/multi-user.target.wants/cardputerzero-early-splash.service"
if [[ $image_profile == recovery && ( -e $early_splash_link || -L $early_splash_link ) ]]; then
    echo "error: recovery image enables the product early splash" >&2
    exit 1
fi
if [[ $access_profile == production ]]; then
    for path in \
        etc/cardputerzero/hardware-debug-access \
        etc/sudoers.d/020-cardputerzero-hardware-debug; do
        if [[ -e $rootfs/$path || -L $rootfs/$path ]]; then
            echo "error: production image contains hardware-debug access: /$path" >&2
            exit 1
        fi
    done
    if find "$rootfs/var/lib/cardputerzero-persist/home" -mindepth 1 \
        -print -quit | grep -q .; then
        echo "error: production image seeds a persistent human home" >&2
        exit 1
    fi
    for unit in regenerate_ssh_host_keys.service \
        getty@.service getty@tty1.service serial-getty@.service \
        serial-getty@serial0.service cardputerzero-recovery-console.service; do
        mask="$rootfs/etc/systemd/system/$unit"
        if [[ ! -L $mask || $(readlink "$mask") != /dev/null ]]; then
            echo "error: production access unit is not masked: $unit" >&2
            exit 1
        fi
    done
    if awk -F: '$3 >= 1000 && $3 < 20000 { found=1 } END { exit !found }' \
        "$rootfs/etc/passwd"; then
        echo "error: production image contains a human account" >&2
        exit 1
    fi
    if grep -q '^cp0-build:' "$rootfs/etc/passwd" "$rootfs/etc/shadow" \
        "$rootfs/etc/group" || find "$rootfs" -xdev -uid 1000 -print -quit | grep -q .; then
        echo "error: production build identity residue remains" >&2
        exit 1
    fi
    if [[ -e $rootfs/etc/systemd/system/multi-user.target.wants/ssh.service ]]; then
        echo "error: production image enables SSH before owner consent" >&2
        exit 1
    fi
    for path in cp0-maintenance.enable cp0-maintenance.authorized_key \
        cp0-maintenance.status; do
        if [[ -e $bootfs/$path || -L $bootfs/$path ]]; then
            echo "error: production image preauthorizes maintenance access: $path" >&2
            exit 1
        fi
    done
    for path in \
        usr/libexec/cardputerzero/prepare-maintenance-ssh.sh \
        usr/libexec/cardputerzero/hot-update-firstboot.sh \
        usr/lib/cardputerzero/maintenance-sshd_config \
        usr/lib/systemd/system/cardputerzero-maintenance-ssh.service \
        etc/systemd/system/multi-user.target.wants/cardputerzero-maintenance-ssh.service; do
        if [[ -e $rootfs/$path || -L $rootfs/$path ]]; then
            echo "error: production image contains pre-Setup remote access: /$path" >&2
            exit 1
        fi
    done
    if grep -q '^host-name=cardputerzero-maintenance$' \
        "$rootfs/etc/avahi/avahi-daemon.conf"; then
        echo "error: production image advertises removed maintenance identity" >&2
        exit 1
    fi
    test -x "$rootfs/usr/lib/systemd/system-generators/cardputerzero-ssh-generator"
    grep -qx 'ConditionPathExists=/var/lib/cardputerzero/provisioning/complete' \
        "$rootfs/usr/lib/systemd/system/ssh.service.d/cardputerzero-gate.conf"
    grep -qx 'ExecCondition=/usr/libexec/cardputerzero/ssh-access-allowed' \
        "$rootfs/usr/lib/systemd/system/ssh.service.d/cardputerzero-gate.conf"
    grep -qx 'AllowGroups cp0-ssh cp0-developer-access' \
        "$rootfs/etc/ssh/sshd_config.d/40-cardputerzero-owner.conf"
    grep -qx 'AuthorizedKeysFile /etc/cardputerzero/authorized_keys/%u' \
        "$rootfs/etc/ssh/sshd_config.d/40-cardputerzero-owner.conf"
    grep -qx 'DisableForwarding yes' \
        "$rootfs/etc/ssh/sshd_config.d/40-cardputerzero-owner.conf"
    grep -qx 'ProtectHome=no' \
        "$rootfs/usr/lib/systemd/system/cardputerzero-provisiond.service"
    grep -qx 'ProtectHostname=no' \
        "$rootfs/usr/lib/systemd/system/cardputerzero-provisiond.service"
    grep -qx 'MemoryMax=64M' \
        "$rootfs/usr/lib/systemd/system/cardputerzero-provisiond.service"
    for database in passwd group shadow; do
        grep -Eq "^${database}:.*(^|[[:space:]])extrausers([[:space:]]|$)" \
            "$rootfs/etc/nsswitch.conf"
    done
    test -e "$rootfs/usr/lib/libnss_extrausers.so.2"
    chroot "$rootfs" /usr/bin/dpkg-query -W -f='${Status}\n' \
        libnss-extrausers | grep -qx 'install ok installed'
    chroot "$rootfs" /usr/bin/locale -a | grep -qx 'en_US.utf8'
    chroot "$rootfs" /usr/bin/locale -a | grep -qx 'zh_CN.utf8'
    for database in passwd shadow group gshadow; do
        if [[ -s $rootfs/var/lib/cardputerzero-persist/extrausers/$database ]]; then
            echo "error: production image seeds an owner identity: $database" >&2
            exit 1
        fi
    done
    if [[ ! -d $rootfs/var/lib/cardputerzero-persist/cardputerzero/provisioning ]]; then
        echo "error: production image omits the persistent provisioning directory" >&2
        exit 1
    fi
    if [[ $(stat -c '%a:%u:%g' \
        "$rootfs/var/lib/cardputerzero-persist/cardputerzero/provisioning") != 700:0:0 ]]; then
        echo "error: persistent provisioning directory ownership or mode is unsafe" >&2
        exit 1
    fi
    chroot "$rootfs" /usr/bin/jq -e \
        '.developer_mode_allowed == true and .recovery_mode_allowed == false' \
        /etc/cardputerzero/device-policy.json >/dev/null
elif [[ ! -L $rootfs/etc/systemd/system/multi-user.target.wants/ssh.service ]]; then
    echo "error: development access does not enable SSH" >&2
    exit 1
fi
grep -qx 'PermitRootLogin no' \
    "$rootfs/etc/ssh/sshd_config.d/40-cardputerzero-owner.conf"
machine_id_commit_mask="$rootfs/etc/systemd/system/systemd-machine-id-commit.service"
if [[ $image_profile == product ]]; then
    if [[ ! -L $machine_id_commit_mask ||
          $(readlink "$machine_id_commit_mask") != /dev/null ]]; then
        echo "error: product image does not mask redundant machine-id commit" >&2
        exit 1
    fi
elif [[ -L $machine_id_commit_mask &&
        $(readlink "$machine_id_commit_mask") == /dev/null ]]; then
    echo "error: recovery image masks machine-id commit" >&2
    exit 1
fi
for path in getty.target.wants/getty@tty1.service \
    multi-user.target.wants/cardputerzero-compositor.service \
    multi-user.target.wants/cardputerzero-recovery-console.service; do
    if [[ -e $rootfs/etc/systemd/system/$path ||
          -L $rootfs/etc/systemd/system/$path ]]; then
        echo "error: display session is statically enabled: $path" >&2
        exit 1
    fi
done
if [[ $image_profile == recovery ]]; then
    masked_units=(
        cardputerzero-compositor.service
        cardputerzero-system-shell.service
        cardputerzero-appd.service
        cardputerzero-appd.socket
        cardputerzero-audiod.socket
        cardputerzero-broker.socket
        cardputerzero-camerad.socket
        cardputerzero-documentd.socket
        cardputerzero-devd.service
        cardputerzero-devd.socket
        cardputerzero-ssh-access.path
        cardputerzero-ssh-access-refresh.service
        cardputerzero-gpiod.socket
        cardputerzero-networkd.socket
        cardputerzero-powerd.service
        cardputerzero-powerd.socket
        cardputerzero-radiod.socket
        cardputerzero-storaged.socket
        cardputerzero-stored.socket
        cardputerzero-usb-mediad.service
        cardputerzero-usb-mediad.socket
    )
    for unit in "${masked_units[@]}"; do
        mask="$rootfs/etc/systemd/system/$unit"
        if [[ ! -L $mask || $(readlink "$mask") != /dev/null ]]; then
            echo "error: recovery image unit is not masked: $unit" >&2
            exit 1
        fi
    done
fi

grep -Rqx 'Storage=volatile' \
    "$rootfs/etc/systemd/journald.conf" \
    "$rootfs/etc/systemd/journald.conf.d"
grep -qE '^tmpfs[[:space:]]+/tmp[[:space:]]+tmpfs' "$rootfs/etc/fstab"
grep -qE '^tmpfs[[:space:]]+/var/tmp[[:space:]]+tmpfs.*size=128M' \
    "$rootfs/etc/fstab"
chroot "$rootfs" /usr/bin/dpkg-query -W -f='${Status}\n' \
    dosfstools | grep -qx 'install ok installed'
grep -qx 'libcomposite' \
    "$rootfs/usr/lib/modules-load.d/cardputerzero-usb-media.conf"
if [[ -e $rootfs/var/lib/cardputerzero/usb-media/exchange.img ||
      -L $rootfs/var/lib/cardputerzero/usb-media/exchange.img ]]; then
    echo "error: image pre-seeds a mutable USB exchange backing file" >&2
    exit 1
fi
grep -qx 'kernel.core_pattern=/dev/null' \
    "$rootfs/etc/sysctl.d/90-cardputerzero-os.conf"
grep -qx 'kernel.unprivileged_bpf_disabled=1' \
    "$rootfs/etc/sysctl.d/90-cardputerzero-os.conf"

test -s "$bootfs/initramfs8"
grep -qx 'auto_initramfs=1' "$bootfs/config.txt"
if [[ $(grep -c '^camera_auto_detect=0$' "$bootfs/config.txt") -ne 1 ]]; then
    echo "error: image does not disable camera auto-detection exactly once" >&2
    exit 1
fi
if [[ $(grep -c '^dtoverlay=imx219$' "$bootfs/config.txt") -ne 1 ]]; then
    echo "error: image does not enable the IMX219 sensor exactly once" >&2
    exit 1
fi
if [[ $(grep -c '^start_x=1$' "$bootfs/config.txt") -ne 1 ]]; then
    echo "error: image does not select the standard camera firmware" >&2
    exit 1
fi
if [[ $(grep -c '^dtoverlay=dwc2,dr_mode=peripheral$' \
    "$bootfs/config.txt") -ne 1 ]]; then
    echo "error: image does not force the USB controller into peripheral mode" >&2
    exit 1
fi
for camera_firmware in start_x.elf fixup_x.dat; do
    if [[ ! -s $bootfs/$camera_firmware ]]; then
        echo "error: image is missing camera firmware: $camera_firmware" >&2
        exit 1
    fi
done
legacy_bootscreen_sha256=d1639763fa6714e2cd4544fb45b9d5e5d54e949eaa11d7e7057651b6d4d51efd
for camera_firmware in start.elf start_x.elf; do
    if [[ $(sha256sum "$bootfs/$camera_firmware" | awk '{print $1}') == \
          "$legacy_bootscreen_sha256" ]]; then
        echo "error: image contains the M5Stack firmware that forces 256 MB GPU memory" >&2
        exit 1
    fi
done
early_splash="$rootfs/usr/share/cardputerzero/boot/splash.rgb565"
early_splash_sha256=75a53d81f5ec087536a030919698c595630d48296e07d5f5f3d04ebebf2efd57
if [[ $(wc -c <"$early_splash") -ne 108800 ||
      $(sha256sum "$early_splash" | awk '{print $1}') != "$early_splash_sha256" ]]; then
    echo "error: image does not contain the pinned 320x170 RGB565 splash" >&2
    exit 1
fi
chroot "$rootfs" /usr/libexec/cardputerzero/early-splash-spi \
    --check-image /usr/share/cardputerzero/boot/splash.rgb565
if grep -qx 'dtoverlay=camera-py12-high-overlay' "$bootfs/config.txt"; then
    echo "error: image conflicts with V0.6 powerfail ownership of P12" >&2
    exit 1
fi
if grep -qx 'dtoverlay=camera-gpio16-high-overlay' "$bootfs/config.txt"; then
    echo "error: image enables the legacy camera GPIO16 overlay on V0.6" >&2
    exit 1
fi
card_line=$(grep -n '^dtoverlay=cardputerzero-v5-overlay$' "$bootfs/config.txt" | cut -d: -f1)
sensor_line=$(grep -n '^dtoverlay=imx219$' "$bootfs/config.txt" | cut -d: -f1)
if [[ -z $card_line || -z $sensor_line || $card_line -ge $sensor_line ]]; then
    echo "error: image camera overlays are not ordered board then sensor" >&2
    exit 1
fi
card_overlay="$bootfs/overlays/cardputerzero-v5-overlay.dtbo"
if [[ ! -s $card_overlay ]]; then
    echo "error: image is missing the CardputerZero V0.6 overlay" >&2
    exit 1
fi
if grep -aFq 'power-supply' "$card_overlay"; then
    echo "error: V0.6 overlay disables PWM instead of driving zero duty" >&2
    exit 1
fi
if [[ $image_profile == product ]]; then
    factory_bundle=/usr/share/cardputerzero/factory-data-v1.cp0backup
    test -s "$rootfs$factory_bundle"
    factory_summary=$(chroot "$rootfs" /usr/bin/cp0-recovery verify "$factory_bundle")
    if [[ $factory_summary != *" profile=product" ]]; then
        echo "error: product factory seed has the wrong profile" >&2
        exit 1
    fi
    if ! grep -qw 'cp0.overlay_root=volatile' "$bootfs/cmdline.txt"; then
        echo "error: immutable root is not enabled in the product image" >&2
        exit 1
    fi
    for token in quiet loglevel=3 logo.nologo vt.global_cursor_default=0 \
        consoleblank=0 fbcon=map:off systemd.show_status=false \
        rd.systemd.show_status=false; do
        if ! grep -Fqw "$token" "$bootfs/cmdline.txt"; then
            echo "error: product image is missing quiet-boot token: $token" >&2
            exit 1
        fi
    done
    for token in loglevel=6 fbcon=map:1 splash; do
        if grep -Fqw "$token" "$bootfs/cmdline.txt"; then
            echo "error: product image retains verbose console token: $token" >&2
            exit 1
        fi
    done
elif grep -qw 'cp0.overlay_root=volatile' "$bootfs/cmdline.txt"; then
    echo "error: recovery image unexpectedly enables immutable root" >&2
    exit 1
fi
if [[ $image_profile == recovery ]]; then
    for token in loglevel=6 consoleblank=0 fbcon=map:1; do
        if ! grep -Fqw "$token" "$bootfs/cmdline.txt"; then
            echo "error: recovery image is missing visible-console token: $token" >&2
            exit 1
        fi
    done
    for token in quiet fbcon=map:off systemd.show_status=false \
        rd.systemd.show_status=false; do
        if grep -Fqw "$token" "$bootfs/cmdline.txt"; then
            echo "error: recovery image incorrectly enables quiet boot: $token" >&2
            exit 1
        fi
    done
fi
if [[ $image_profile == recovery &&
      -e $rootfs/usr/share/cardputerzero/factory-data-v1.cp0backup ]]; then
    echo "error: recovery image contains an incomplete product factory seed" >&2
    exit 1
fi
if grep -qw 'resize' "$bootfs/cmdline.txt"; then
    echo "error: upstream root resize would overwrite cp0-data" >&2
    exit 1
fi
initramfs_contents=$(chroot "$rootfs" \
    /usr/bin/lsinitramfs /boot/firmware/initramfs8)
grep -qx 'scripts/local-premount/cardputerzero-data-grow' \
    <<<"$initramfs_contents"
grep -qx 'scripts/init-bottom/cardputerzero-overlay-root' \
    <<<"$initramfs_contents"
grep -qE 'usr/lib/modules/.*/kernel/fs/overlayfs/overlay\.ko' \
    <<<"$initramfs_contents"
grep -qx 'usr/lib/firmware/cardputerzero,st7789v_lcd.bin' \
    <<<"$initramfs_contents"
if [[ $image_profile == product ]]; then
    for path in \
        scripts/init-top/cardputerzero-early-splash \
        usr/libexec/cardputerzero/early-splash-spi \
        usr/libexec/cardputerzero/show-early-splash.sh \
        usr/share/cardputerzero/boot/splash.rgb565; do
        grep -qx "$path" <<<"$initramfs_contents"
    done
else
    for path in \
        scripts/init-top/cardputerzero-early-splash \
        usr/libexec/cardputerzero/early-splash-spi \
        usr/share/cardputerzero/boot/splash.rgb565; do
        if grep -qx "$path" <<<"$initramfs_contents"; then
            echo "error: recovery initramfs contains product splash path: $path" >&2
            exit 1
        fi
    done
fi

verify_tmp_parent="$rootfs/run"
initramfs_extract=$(mktemp -d \
    "$verify_tmp_parent/cardputerzero-initramfs-order.XXXXXX")
case "$initramfs_extract" in
    "$verify_tmp_parent"/cardputerzero-initramfs-order.*) ;;
    *)
        echo "error: unsafe initramfs verification directory" >&2
        exit 1
        ;;
esac
trap 'rm -rf -- "$initramfs_extract"' EXIT
initramfs_chroot=${initramfs_extract#"$rootfs"}
case "$initramfs_chroot" in
    /run/cardputerzero-initramfs-order.*) ;;
    *)
        echo "error: initramfs verification directory is outside rootfs" >&2
        exit 1
        ;;
esac
chroot "$rootfs" /usr/bin/unmkinitramfs \
    /boot/firmware/initramfs8 "$initramfs_chroot"
grep -Fqx '/scripts/init-bottom/cardputerzero-overlay-root "$@"' \
    "$initramfs_extract/scripts/init-bottom/ORDER"
grep -Fqx '/scripts/local-premount/cardputerzero-data-grow "$@"' \
    "$initramfs_extract/scripts/local-premount/ORDER"
if [[ $image_profile == product ]]; then
    grep -Fqx '/scripts/init-top/cardputerzero-early-splash "$@"' \
        "$initramfs_extract/scripts/init-top/ORDER"
fi

generator_output="$initramfs_extract/display-generator"
generator_chroot="$initramfs_chroot/display-generator"
mkdir -p "$generator_output/early" "$generator_output/late"
chroot "$rootfs" \
    /usr/lib/systemd/system-generators/cardputerzero-display-generator \
    "$generator_chroot" "$generator_chroot/early" "$generator_chroot/late"
if [[ $image_profile == product ]]; then
    selected_display=cardputerzero-compositor.service
else
    selected_display=cardputerzero-recovery-console.service
fi
test -L "$generator_output/multi-user.target.wants/$selected_display"
if [[ $(find "$generator_output/multi-user.target.wants" -type l | wc -l) -ne 1 ]]; then
    echo "error: display generator did not select exactly one session" >&2
    exit 1
fi

data_root="$rootfs/var/lib/cardputerzero-persist"
if [[ $(findmnt -n -o FSTYPE --target "$data_root") != ext4 ]]; then
    echo "error: cp0-data is not mounted during image verification" >&2
    exit 1
fi
data_device=$(findmnt -n -o SOURCE --target "$data_root")
if [[ $(blkid -s LABEL -o value "$data_device") != cp0-data ]]; then
    echo "error: persistent filesystem label is not cp0-data" >&2
    exit 1
fi
grep -qx 'cp0-data-layout-v2' "$data_root/layout-version"
grep -qx "$image_profile" \
    "$data_root/etc-cardputerzero/image-profile"
grep -qx "$access_profile" \
    "$data_root/etc-cardputerzero/access-profile"
for path in cardputerzero etc-cardputerzero extrausers home \
    network-connections network-state ssh; do
    if [[ ! -d $data_root/$path || -L $data_root/$path ]]; then
        echo "error: persistent layout directory is invalid: $path" >&2
        exit 1
    fi
done
for path in machine-id random-seed; do
    if [[ ! -f $data_root/$path || -L $data_root/$path ]]; then
        echo "error: persistent layout file is invalid: $path" >&2
        exit 1
    fi
done
for database in passwd group shadow gshadow; do
    if [[ ! -f $data_root/extrausers/$database ||
          -L $data_root/extrausers/$database ]]; then
        echo "error: persistent owner database is invalid: $database" >&2
        exit 1
    fi
done

if [[ -e $rootfs/etc/apt/apt.conf.d/51cache ]]; then
    echo "error: build proxy configuration leaked into the image" >&2
    exit 1
fi
if grep -R -E \
    '^[[:space:]]*(deb[[:space:]]+|URIs:[[:space:]]*)http://(deb\.debian\.org|archive\.raspberrypi\.com)' \
    "$rootfs/etc/apt/sources.list" "$rootfs/etc/apt/sources.list.d" \
    2>/dev/null; then
    echo "error: unencrypted Debian or Raspberry Pi apt source in image" >&2
    exit 1
fi
if find "$rootfs" -xdev -type f -perm /0002 -print -quit | grep -q .; then
    echo "error: image contains a world-writable regular file" >&2
    exit 1
fi
if find "$data_root" -xdev -type f -perm /0002 -print -quit | grep -q .; then
    echo "error: cp0-data contains a world-writable regular file" >&2
    exit 1
fi

echo "PASS built rootfs and initramfs profile: $image_profile"
