#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh"
packages="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/00-packages-nr"
service="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/cardputerzero-compositor.service"
launcher="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/start-compositor.sh"
config="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/files/weston.ini"
version="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/weston.env"

grep -Eq '^WESTON_COMMIT=[0-9a-f]{40}$' "$version"
grep -q -- '-Dbackend-drm=true' "$stage"
grep -q -- '-Dbackend-headless=true' "$stage"
grep -q -- '-Drenderer-gl=false' "$stage"
grep -q -- '-Dxwayland=false' "$stage"
grep -q -- '-Dbackend-rdp=false' "$stage"
grep -q -- '-Dbackend-vnc=false' "$stage"
grep -q -- '-Dpipewire=false' "$stage"
grep -q -- '-Dshell-kiosk=true' "$stage"
grep -q '/usr/libexec/cardputerzero/start-compositor.sh' "$service"
grep -q '/dev/dri/cardputer-zero-internal' "$launcher"
grep -q -- '--seat=seat-cardputer-zero' "$launcher"
grep -q -- '--renderer=pixman' "$launcher"
grep -q '^Conflicts=getty@tty1.service$' "$service"
grep -q '^OnFailure=getty@tty1.service$' "$service"
grep -q '^mode=320x170@30$' "$config"
grep -qx 'seatd' "$packages"
sh -n "$launcher"

for package in pipewire xwayland weston; do
    if grep -qx "$package" "$packages"; then
        echo "error: prohibited generic compositor dependency: $package" >&2
        exit 1
    fi
done
