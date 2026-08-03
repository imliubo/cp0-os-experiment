#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build="$repo_root/image/build-image.sh"
bsp="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
banner="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/console-banner.sh"
compositor="$repo_root/image/pi-gen/stage-cardputerzero-os/01-compositor/01-run.sh"
platform="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"
export_profile="$repo_root/image/pi-gen/stage-cardputerzero-os/EXPORT_IMAGE"
verifier="$repo_root/tests/test-built-rootfs-profile.sh"
mkdir -p "$repo_root/target/test-tmp"
invalid_log=$(mktemp "$repo_root/target/test-tmp/recovery-profile-invalid.XXXXXX")
trap 'rm -f "$invalid_log"' EXIT

if CP0_IMAGE_PROFILE=invalid "$build" >"$invalid_log" 2>&1; then
    echo "error: build accepted an invalid image profile" >&2
    exit 1
fi
grep -q 'CP0_IMAGE_PROFILE must be product or recovery' \
    "$invalid_log"

grep -q 'image_profile=${CP0_IMAGE_PROFILE:-product}' "$build"
grep -q 'stage-cardputerzero-os/image-profile' "$build"
grep -q 'a recovery image cannot embed a Store trust key' "$build"
for stage_script in "$bsp" "$compositor" "$platform"; do
    grep -Fq '${STAGE_DIR}/image-profile' "$stage_script"
    if grep -Fq '${STAGE_DIR}/../image-profile' "$stage_script"; then
        echo "error: image profile is read outside the pi-gen stage root" >&2
        exit 1
    fi
done
grep -q '/etc/cardputerzero/image-profile' "$bsp"
grep -q '\$image_profile == product' "$bsp"
grep -Fq 'cp0\.overlay_root=volatile' "$bsp"
grep -q 'profile=RECOVERY' "$banner"
grep -q 'systemctl mask cardputerzero-compositor.service' "$compositor"
grep -q 'cardputerzero-system-shell.service' "$compositor"
grep -q 'systemctl mask cardputerzero-appd.service' "$platform"
grep -q 'cardputerzero-powerd.service cardputerzero-powerd.socket' "$platform"
grep -q 'cardputerzero-stored.socket' "$platform"
grep -q 'cp0-os-recovery' "$export_profile"
grep -q 'IMAGE_PROFILE=.*image-profile' "$export_profile"
grep -q 'image_profile == recovery' "$verifier"
grep -q 'recovery image unit is not masked' "$verifier"
grep -q 'recovery image unexpectedly enables immutable root' "$verifier"

recovery_branch() {
    awk '
        /^if \[\[ \$image_profile == product \]\]; then$/ { profile = 1; next }
        profile && /^else$/ { recovery = 1; next }
        recovery && /^fi$/ { exit }
        recovery { print }
    ' "$1"
}
if grep -Eq 'systemctl (enable|start) cardputerzero-(compositor|appd)' \
    <(recovery_branch "$compositor") \
    <(recovery_branch "$platform"); then
    echo "error: recovery branch enables an application execution entry point" >&2
    exit 1
fi
