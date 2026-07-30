#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh"
packages="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/00-packages-nr"
service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-compositor.service"
shell_service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-system-shell.service"
launcher="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/start-compositor.sh"
waiter="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/wait-wayland.sh"
unblanker="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/unblank-display.sh"
config="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/weston.ini"
version="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/weston.env"
policy="$repo_root/compositor-policy/cardputerzero-policy.c"
protocol="$repo_root/protocols/cardputerzero-system-shell-v1.xml"
shell_client="$repo_root/system-shell/src/main.c"

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
grep -q 'cardputerzero-system-shell-v1.xml' "$repo_root/image/build-image.sh"
grep -q '^modules=cardputerzero-policy.so$' "$config"
grep -q '/usr/libexec/cardputerzero/start-compositor.sh' "$service"
grep -q '^ExecStartPost=+/usr/libexec/cardputerzero/unblank-display.sh$' "$service"
grep -q 'files/unblank-display.sh' "$stage"
grep -q '^Wants=cardputerzero-system-shell.service$' "$service"
grep -q '/usr/bin/cardputerzero-system-shell' "$shell_service"
grep -q '^Restart=always$' "$shell_service"
grep -q '^MemoryMax=32M$' "$shell_service"
grep -q '^User=cp0-compositor$' "$service"
grep -q '^Group=cp0-wayland$' "$service"
grep -q '^RuntimeDirectoryMode=0770$' "$service"
grep -q '^UMask=0007$' "$service"
grep -q '^User=cp0-shell$' "$shell_service"
grep -q '^SupplementaryGroups=cp0-wayland$' "$shell_service"
grep -q 'groupadd --system cp0-wayland' "$stage"
grep -q 'cp0-compositor' "$stage"
grep -q 'usermod -G cp0-wayland cp0-shell' "$stage"
if grep -q 'usermod -a -G cp0-wayland cp0-shell' "$stage"; then
    echo "error: existing cp0-shell hardware groups would be retained" >&2
    exit 1
fi
grep -q 'system-shell/include/cp0_ui.h' "$repo_root/image/build-image.sh"
grep -q 'cardputerzero-system-shell/main.c' "$stage"
grep -q '/dev/dri/cardputer-zero-internal' "$launcher"
grep -q -- '--seat=seat-cardputer-zero' "$launcher"
grep -q -- '--renderer=pixman' "$launcher"
grep -q '^Conflicts=getty@tty1.service$' "$service"
grep -q '^OnFailure=getty@tty1.service$' "$service"
grep -q '^mode=320x170@30$' "$config"
grep -qx 'seatd' "$packages"
sh -n "$launcher"
sh -n "$waiter"
sh -n "$unblanker"
grep -q '/dev/fb_lcd' "$unblanker"
grep -q '/sys/class/graphics/\$fb_device/blank' "$unblanker"
grep -q "printf '0" "$unblanker"
grep -q 'wl_client_get_credentials' "$policy"
grep -q 'uid != policy->trusted_uid' "$policy"
grep -q 'WESTON_LAYER_POSITION_TOP_UI' "$policy"
grep -q 'WESTON_LAYER_POSITION_HIDDEN' "$policy"
grep -q 'weston_compositor_add_key_binding' "$policy"
grep -q 'cp0_system_shell_v1_send_action' "$policy"
grep -q 'cp0_system_shell_v1_register_surface' "$shell_client"
grep -q '<interface name="cp0_system_shell_v1" version="1">' "$protocol"

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
