#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
packages="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/00-packages-nr"
build_script="$repo_root/image/build-image.sh"

grep -q '^PI_GEN_BRANCH=arm64$' "$repo_root/image/pi-gen/upstream.env"
grep -Eq '^PI_GEN_COMMIT=[0-9a-f]{40}$' "$repo_root/image/pi-gen/upstream.env"
grep -q '^dtoverlay=vc4-kms-v3d,cma-64$' "$stage"
grep -q '^gpu_mem=64$' "$stage"
grep -q '^gpu_mem_512=64$' "$stage"
grep -q 'systemctl set-default multi-user.target' "$stage"
grep -q '/pi-gen/stage0 /pi-gen/stage1 /pi-gen/stage-cardputerzero-os' "$build_script"
grep -q 'export-image/01-user-rename/SKIP' "$build_script"
grep -q 'CP0_RESUME_BUILD' "$build_script"
grep -q -- '--volumes-from' "$build_script"
if grep -q '/pi-gen/stage2' "$build_script"; then
    echo "error: stage2 must not be part of the minimal image" >&2
    exit 1
fi

for package in lightdm wayfire wf-panel-pi pcmanfm packagekit pipewire; do
    if grep -qx "$package" "$packages"; then
        echo "error: prohibited GUI package in minimal image: $package" >&2
        exit 1
    fi
    grep -qw "$package" "$stage"
done
