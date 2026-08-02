#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh"
packages="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/00-packages-nr"
service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-compositor.service"
shell_service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-system-shell.service"
recovery_service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-recovery-console.service"
display_retry_service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-display-retry.service"
display_retry="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/retry-display-once.sh"
display_generator="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-display-generator"
launcher="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/start-compositor.sh"
waiter="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/wait-wayland.sh"
unblanker="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/unblank-display.sh"
udev_rules="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/99-cardputerzero-systemd.rules"
config="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/weston.ini"
version="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/weston.env"
policy="$repo_root/compositor-policy/cardputerzero-policy.c"
esc_gesture="$repo_root/compositor-policy/esc-gesture.c"
esc_gesture_header="$repo_root/compositor-policy/esc-gesture.h"
esc_gesture_test="$repo_root/tests/compositor-esc-gesture.c"
protocol="$repo_root/protocols/cardputerzero-system-shell-v1.xml"
shell_client="$repo_root/system-shell/src/main.c"
provision_client="$repo_root/system-shell/src/provision_client.c"
builder="$repo_root/scripts/build-compositor.sh"
builder_image="$repo_root/containers/compositor-builder/Containerfile"
installer="$repo_root/scripts/device-install-compositor.sh"

grep -Eq '^WESTON_COMMIT=[0-9a-f]{40}$' "$version"
grep -q -- '-Dbackend-drm=true' "$stage"
grep -q -- '-Dbackend-headless=true' "$stage"
grep -q -- '-Drenderer-gl=false' "$stage"
grep -q -- '-Dxwayland=false' "$stage"
grep -q -- '-Dbackend-rdp=false' "$stage"
grep -q -- '-Dbackend-vnc=false' "$stage"
grep -q -- '-Dpipewire=false' "$stage"
grep -q -- '-Dshell-kiosk=true' "$stage"
grep -q 'cardputerzero-policy.so' "$stage"
grep -q -- '-Wl,-z,defs' "$stage"
grep -q 'pkg-config --cflags --libs pixman-1 wayland-server' "$stage"
grep -q 'pkg-config --cflags --libs wayland-client libpng libdrm xkbcommon' "$stage"
grep -q 'xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_SHIFT)' "$shell_client"
grep -q 'mods_depressed & shell->shift_modifier_mask' "$shell_client"
if grep -q 'mods_depressed | mods_latched | mods_locked' "$shell_client"; then
    echo "error: text input treats latched or locked Shift as physically held" >&2
    exit 1
fi
grep -q '^#define CP0_PROVISION_TIMEOUT_PASSWORD 75$' "$provision_client"
grep -q '^#define CP0_PROVISION_TIMEOUT_SYSTEM 20$' "$provision_client"
grep -q '^#define CP0_PROVISION_TIMEOUT_WIFI_SCAN 45$' "$provision_client"
grep -q '^#define CP0_PROVISION_TIMEOUT_WIFI_CONNECT 75$' "$provision_client"
if grep -q 'CP0_PROVISION_TIMEOUT_QUICK' "$provision_client"; then
    echo "first-boot state responses must allow bounded live network probes" >&2
    exit 1
fi
grep -q 'libpng-dev' "$builder_image"
grep -q 'containers/compositor-builder/Containerfile' "$repo_root/Makefile"
grep -q 'cardputerzero-system-shell-v1.xml' "$repo_root/image/build-image.sh"
grep -q '^modules=cardputerzero-policy.so$' "$config"
grep -q '/usr/libexec/cardputerzero/start-compositor.sh' "$service"
grep -q '^ExecStartPost=+/usr/libexec/cardputerzero/unblank-display.sh$' "$service"
grep -q 'files/unblank-display.sh' "$stage"
grep -q '^Wants=cardputerzero-system-shell.service$' "$service"
grep -q '^After=cardputerzero-system-shell.service$' "$display_retry_service"
grep -q '^Type=oneshot$' "$display_retry_service"
grep -q '^RemainAfterExit=yes$' "$display_retry_service"
grep -q '^WantedBy=multi-user.target$' "$display_retry_service"
grep -q 'systemctl restart cardputerzero-compositor.service' "$display_retry"
grep -q 'cardputerzero-display-retry.service' "$stage"
grep -q '^delay=${CP0_DISPLAY_RETRY_DELAY:-8}$' "$display_retry"
grep -q '/usr/bin/cardputerzero-system-shell' "$shell_service"
grep -q '^Wants=cardputerzero-powerd.socket cardputerzero-provisiond.socket$' "$shell_service"
grep -q '^After=cardputerzero-compositor.service cardputerzero-powerd.socket cardputerzero-provisiond.socket$' "$shell_service"
grep -q '^SupplementaryGroups=.*cp0-power-control' "$shell_service"
grep -q '^Restart=always$' "$shell_service"
grep -q '^MemoryMax=32M$' "$shell_service"
grep -q '^StateDirectory=cardputerzero/screenshots cardputerzero/shell$' "$shell_service"
grep -q '^StateDirectoryMode=0750$' "$shell_service"
grep -q '^MemoryMax=32M$' "$service"
grep -q '^MemorySwapMax=0$' "$service"
grep -q '^TasksMax=32$' "$service"
grep -q '^User=cp0-compositor$' "$service"
grep -q '^Group=cp0-wayland$' "$service"
grep -q '^RuntimeDirectoryMode=0770$' "$service"
grep -q '^UMask=0007$' "$service"
grep -q '^CapabilityBoundingSet=$' "$service"
grep -q '^ProtectKernelModules=yes$' "$service"
grep -q '^ProtectProc=invisible$' "$service"
grep -q '^RestrictAddressFamilies=AF_UNIX AF_NETLINK$' "$service"
grep -q '^MemoryDenyWriteExecute=yes$' "$service"
grep -q '^User=cp0-shell$' "$shell_service"
grep -q '^SupplementaryGroups=cp0-wayland cp0-control cp0-display-control cp0-audio-control cp0-connectivity-control cp0-power-control cp0-provision-control$' "$shell_service"
grep -q '^MemorySwapMax=0$' "$shell_service"
grep -q '^PrivateDevices=yes$' "$shell_service"
grep -q '^ProtectKernelModules=yes$' "$shell_service"
grep -q '^RestrictNamespaces=yes$' "$shell_service"
grep -q '^MemoryDenyWriteExecute=yes$' "$shell_service"
grep -q '^ProtectProc=invisible$' "$shell_service"
grep -q '^RestrictAddressFamilies=AF_UNIX AF_NETLINK$' "$shell_service"
if grep -q '^ProcSubset=pid$' "$shell_service"; then
    echo 'error: System Shell telemetry requires the system-wide proc subset' >&2
    exit 1
fi
grep -q 'groupadd --system cp0-wayland' "$stage"
grep -q 'cp0-compositor' "$stage"
grep -q 'usermod -G cp0-wayland cp0-shell' "$stage"
if grep -q 'usermod -a -G cp0-wayland cp0-shell' "$stage"; then
    echo "error: existing cp0-shell hardware groups would be retained" >&2
    exit 1
fi
grep -q 'system-shell/include/cp0_ui.h' "$repo_root/image/build-image.sh"
grep -q 'system-shell/include/cp0_store_client.h' "$repo_root/image/build-image.sh"
grep -q 'system-shell/include/cp0_provision_client.h' "$repo_root/image/build-image.sh"
grep -q 'system-shell/include/cp0_shell_settings.h' "$repo_root/image/build-image.sh"
grep -q 'cardputerzero-system-shell/main.c' "$stage"
grep -q 'cardputerzero-system-shell/store_client.c' "$stage"
grep -q 'cardputerzero-system-shell/provision_client.c' "$stage"
grep -q 'cardputerzero-system-shell/shell_settings.c' "$stage"
grep -q '/dev/dri/cardputer-zero-internal' "$launcher"
grep -q -- '--seat=seat-cardputer-zero' "$launcher"
grep -q -- '--renderer=pixman' "$launcher"
grep -q '^Conflicts=getty@tty1.service$' "$service"
grep -q '^OnFailure=getty@tty1.service$' "$service"
grep -Fq 'dev-dri-cardputer\x2dzero\x2dinternal.device' "$service"
grep -Fq 'dev-input-cardputer\x2dzero\x2dinternal.device' "$service"
grep -q '^JobTimeoutSec=30s$' "$service"
if grep -q '^ConditionPathExists=/dev/' "$service"; then
    echo "error: compositor hardware conditions race udev coldplug" >&2
    exit 1
fi
grep -q 'TAG+="systemd"' "$udev_rules"
grep -q 'files/99-cardputerzero-systemd.rules' "$stage"
grep -q '^Conflicts=cardputerzero-compositor.service$' "$recovery_service"
grep -q '^Wants=getty@tty1.service$' "$recovery_service"
if grep -q '^WantedBy=' "$recovery_service"; then
    echo "error: recovery console must only be selected by the display generator" >&2
    exit 1
fi
grep -q 'systemctl disable getty@tty1.service cardputerzero-compositor.service' "$stage"
if grep -Eq 'systemctl enable (getty@tty1|cardputerzero-(compositor|recovery-console))' "$stage"; then
    echo "error: display sessions cannot be enabled outside the display generator" >&2
    exit 1
fi
grep -q '^RequiresMountsFor=/var/lib/cardputerzero/registry$' "$service"
grep -q '^ConditionPathExists=!/var/lib/cardputerzero/registry/recovery-mode$' "$service"
grep -q '^mode=320x170@30$' "$config"
grep -qx 'seatd' "$packages"
grep -qx 'libpng16-16t64' "$packages"
sh -n "$launcher"
sh -n "$waiter"
sh -n "$unblanker"
sh -n "$display_retry"
sh -n "$display_generator"
mkdir -p "$repo_root/target/test-tmp"
generator_tmp=$(mktemp -d "$repo_root/target/test-tmp/display-generator.XXXXXX")
trap 'rm -rf -- "$generator_tmp"' EXIT
"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/compositor-policy" \
    "$esc_gesture" "$esc_gesture_test" \
    -o "$generator_tmp/compositor-esc-gesture-test"
"$generator_tmp/compositor-esc-gesture-test"
marker="$generator_tmp/recovery-mode"
test "$("$display_generator" --select product "$marker")" = \
    cardputerzero-compositor.service
touch "$marker"
test "$("$display_generator" --select product "$marker")" = \
    cardputerzero-recovery-console.service
rm -f "$marker"
ln -s invalid "$marker"
test "$("$display_generator" --select product "$marker")" = \
    cardputerzero-recovery-console.service
test "$("$display_generator" --select recovery "$marker")" = \
    cardputerzero-recovery-console.service
test "$("$display_generator" --select invalid "$marker")" = \
    cardputerzero-recovery-console.service
grep -q '/dev/fb_lcd' "$unblanker"
grep -q '/sys/class/graphics/\$fb_device/blank' "$unblanker"
grep -q "printf '0" "$unblanker"
grep -q 'wl_client_get_credentials' "$policy"
grep -q 'uid != policy->trusted_uid' "$policy"
grep -q 'WESTON_LAYER_POSITION_TOP_UI' "$policy"
grep -q 'WESTON_LAYER_POSITION_NORMAL' "$policy"
grep -q 'WESTON_LAYER_POSITION_HIDDEN' "$policy"
grep -q 'weston_compositor_add_key_binding' "$policy"
grep -q 'cp0_system_shell_v1_send_action' "$policy"
grep -q 'wl_event_loop_add_timer' "$policy"
grep -q 'keyboard_has_key(keyboard, KEY_ESC)' "$policy"
grep -q '^#define CP0_ESC_LONG_PRESS_MSEC 800U$' "$esc_gesture_header"
grep -q 'CP0_ESC_GESTURE_HOME' "$esc_gesture"
grep -q 'weston_compositor_add_screenshot_authority' "$policy"
grep -q 'attempt->authorized = true' "$policy"
grep -q 'attempt->denied = true' "$policy"
grep -q 'attempt->who->client == shell_client' "$policy"
grep -q 'attempt->who->output->width == 320' "$policy"
grep -q 'cp0_system_shell_v1_register_surface' "$shell_client"
grep -q 'cp0_system_shell_v1_activate_app' "$shell_client"
grep -q '<interface name="cp0_system_shell_v1" version="7">' "$protocol"
grep -q '<event name="app_identity" since="7">' "$protocol"
grep -q '<entry name="brightness_down" value="4" since="5"/>' "$protocol"
grep -q '<entry name="screenshot" value="13" since="5"/>' "$protocol"
for key in KEY_BRIGHTNESSDOWN KEY_BRIGHTNESSUP KEY_MUTE KEY_VOLUMEDOWN \
    KEY_VOLUMEUP KEY_PLAYPAUSE KEY_PREVIOUSSONG KEY_NEXTSONG KEY_HELP KEY_SYSRQ
do
    grep -q "add_system_binding(policy, $key)" "$policy"
done
for key in KEY_F KEY_Z KEY_X KEY_C
do
    grep -q "case $key:" "$shell_client"
done
for key in KEY_HOME KEY_PAGEUP KEY_PAGEDOWN KEY_INSERT KEY_END KEY_DELETE \
    KEY_F5 KEY_F6 KEY_F7 KEY_F8 KEY_F9 KEY_F10 KEY_F11 KEY_F12
do
    if grep -q "add_system_binding(policy, $key);" "$policy"; then
        echo "foreground key is incorrectly registered as global: $key" >&2
        exit 1
    fi
done
if grep -q 'case KEY_HOME:' "$shell_client"; then
    echo 'Fn+K Home is incorrectly translated to OS Home by the Shell' >&2
    exit 1
fi
grep -q '<request name="activate_app" since="2">' "$protocol"
grep -q '<request name="set_overlay_mode" since="3">' "$protocol"
grep -q '<event name="app_display_mode" since="3">' "$protocol"
grep -q '<request name="sleep_display" since="3">' "$protocol"
grep -q '<request name="set_idle_timeout" since="6">' "$protocol"
grep -q '<entry name="notification" value="3" since="4"' "$protocol"
grep -q '^#define CP0_APP_ID_MAX 128$' "$policy"
grep -q 'WL_SHM_FORMAT_ARGB8888' "$shell_client"
grep -q 'DRM_FORMAT_ARGB8888' "$shell_client"
grep -q 'weston_capture_source_v1_capture' "$shell_client"
grep -q 'CP0_SCREENSHOT_DIRECTORY' "$shell_client"
grep -q 'weston-output-capture-protocol.c' "$builder"
grep -q 'screenshot_store.c' "$builder"
grep -q 'display_client.c' "$builder"
grep -q 'audio_settings_client.c' "$builder"
grep -q 'compositor-policy/esc-gesture.c' "$builder"
grep -q 'compositor-policy/esc-gesture.c' "$repo_root/image/build-image.sh"
grep -q '/tmp/cardputerzero-policy/esc-gesture.c' "$stage"
grep -q 'CP0_SYSTEM_SHELL_V1_OVERLAY_MODE_STATUS' "$policy"
grep -q 'weston_compositor_sleep' "$policy"
grep -q 'compositor->wake_signal' "$policy"
grep -q 'WESTON_COMMIT' "$builder"
grep -q -- '-Wl,-z,defs' "$builder"
grep -q 'systemctl stop cardputerzero-compositor.service' "$installer"
grep -q 'cardputerzero-app-runtime' "$installer"
sh -n "$installer"

if command -v xmllint >/dev/null 2>&1; then
    xmllint --noout "$protocol"
fi

if grep -q -- '-- weston-simple-shm' "$launcher"; then
    echo "error: diagnostic SHM client remains in the production start path" >&2
    exit 1
fi

for package in pipewire xwayland weston; do
    if grep -qx "$package" "$packages"; then
        echo "error: prohibited generic compositor dependency: $package" >&2
        exit 1
    fi
done
