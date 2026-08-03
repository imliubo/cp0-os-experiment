#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/weston.env"

if [ "$(uname -m)" != aarch64 ]; then
    echo "error: compositor build must run in the ARM64 builder" >&2
    exit 1
fi

weston_source="$repo_root/target/vendor/weston"
weston_build="$repo_root/target/weston-minimal/build"
output="$repo_root/target/compositor-aarch64"
protocol="$repo_root/protocols/cardputerzero-system-shell-v1.xml"

actual_commit=$(git -C "$weston_source" rev-parse HEAD)
if [ "$actual_commit" != "$WESTON_COMMIT" ]; then
    echo "error: Weston checkout is $actual_commit, expected $WESTON_COMMIT" >&2
    exit 1
fi
if [ ! -f "$weston_build/config.h" ] ||
    [ ! -f "$weston_build/libweston/libweston-14.so" ]; then
    echo "error: pinned Weston build artifacts are unavailable" >&2
    exit 1
fi

mkdir -p "$output"
wayland-scanner client-header "$protocol" \
    "$output/cardputerzero-system-shell-client-protocol.h"
wayland-scanner server-header "$protocol" \
    "$output/cardputerzero-system-shell-server-protocol.h"
wayland-scanner private-code "$protocol" \
    "$output/cardputerzero-system-shell-protocol.c"

cc -std=c11 -Os -Wall -Wextra -Werror \
    -I"$repo_root/system-shell/include" \
    -I"$output" \
    -I"$weston_build/protocol" \
    "$repo_root/system-shell/src/main.c" \
    "$repo_root/system-shell/src/ui.c" \
    "$repo_root/system-shell/src/screenshot_store.c" \
    "$repo_root/system-shell/src/shell_settings.c" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/appd_client.c" \
    "$repo_root/system-shell/src/audio_settings_client.c" \
    "$repo_root/system-shell/src/connectivity_client.c" \
    "$repo_root/system-shell/src/provision_client.c" \
    "$repo_root/system-shell/src/display_client.c" \
    "$repo_root/system-shell/src/developer_client.c" \
    "$repo_root/system-shell/src/store_client.c" \
    "$repo_root/system-shell/src/system_info.c" \
    "$output/cardputerzero-system-shell-protocol.c" \
    "$weston_build/protocol/xdg-shell-protocol.c" \
    "$weston_build/protocol/weston-output-capture-protocol.c" \
    $(pkg-config --cflags --libs wayland-client libpng libdrm xkbcommon) \
    -o "$output/cardputerzero-system-shell"

cc -std=c11 -Os -Wall -Wextra -Werror -fPIC -shared -Wl,-z,defs \
    -I"$output" \
    -I"$weston_source" \
    -I"$weston_source/include" \
    -I"$weston_build" \
    "$repo_root/compositor-policy/cardputerzero-policy.c" \
    "$repo_root/compositor-policy/esc-gesture.c" \
    "$output/cardputerzero-system-shell-protocol.c" \
    -L"$weston_build/libweston" -lweston-14 \
    $(pkg-config --cflags --libs pixman-1 wayland-server) \
    -o "$output/cardputerzero-policy.so"

file "$output/cardputerzero-system-shell" | grep -q 'ELF 64-bit LSB.*ARM aarch64'
file "$output/cardputerzero-policy.so" | grep -q 'ELF 64-bit LSB.*ARM aarch64'
sha256sum "$output/cardputerzero-system-shell" "$output/cardputerzero-policy.so"
