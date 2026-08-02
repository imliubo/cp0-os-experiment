#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build="$repo_root/image/build-image.sh"
bsp="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
platform="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"
export_profile="$repo_root/image/pi-gen/stage-cardputerzero-os/EXPORT_IMAGE"
verifier="$repo_root/tests/test-built-rootfs-profile.sh"
policy="$repo_root/appd/device-policy-production.json"
mkdir -p "$repo_root/target/test-tmp"
test_root=$(mktemp -d "$repo_root/target/test-tmp/production-access.XXXXXX")
trap 'rm -rf -- "$test_root"' EXIT

expect_rejected() {
    local expected=$1
    shift
    local output="$test_root/rejected-$(find "$test_root" -type f | wc -l).log"
    if env "$@" "$build" >"$output" 2>&1; then
        echo "error: invalid production access build was accepted" >&2
        exit 1
    fi
    grep -Fq "$expected" "$output"
}

expect_rejected \
    'CP0_ACCESS_PROFILE must be development or production' \
    CP0_ACCESS_PROFILE=invalid
expect_rejected \
    'production images reject CP0_FIRST_USER_PASSWORD' \
    CP0_ACCESS_PROFILE=production CP0_FIRST_USER_PASSWORD=shared
expect_rejected \
    'production images reject CP0_SSH_PUBLIC_KEY' \
    CP0_ACCESS_PROFILE=production CP0_SSH_PUBLIC_KEY='ssh-ed25519 test'
expect_rejected \
    'recovery images require the development access profile' \
    CP0_IMAGE_PROFILE=recovery CP0_ACCESS_PROFILE=production

jq -e \
    '.schema_version == 1 and
     .authority == "personal" and
     .developer_mode_allowed == false and
     .recovery_mode_allowed == false and
     .store_install_allowed == true' \
    "$policy" >/dev/null

grep -Fq 'access_profile=${CP0_ACCESS_PROFILE:-development}' "$build"
grep -Fq 'stage-cardputerzero-os/access-profile' "$build"
grep -Fq 'openssl rand -hex 32' "$build"
grep -Fq 'FIRST_USER_NAME=cp0-build' "$build"
grep -Fq 'device-policy-production.json' "$build"
grep -Fq '/etc/cardputerzero/access-profile' "$bsp"
grep -Fq 'userdel --remove "$FIRST_USER_NAME"' "$bsp"
grep -Fq 'temporary product build identity remains' "$bsp"
grep -Fq 'test -e /usr/lib/libnss_extrausers.so.2' "$bsp"
grep -Fq 'locale-gen en_US.UTF-8 zh_CN.UTF-8' "$bsp"
if grep -Fq 'libpam-extrausers' \
    "$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/00-packages-nr"; then
    echo "error: unavailable Debian package libpam-extrausers is requested" >&2
    exit 1
fi
if grep -Fq 'systemctl mask --force ssh.service ssh.socket' "$bsp"; then
    echo "error: owner-controlled SSH cannot be permanently masked" >&2
    exit 1
fi
grep -Fq 'serial-getty@.service' "$bsp"
grep -Fq 'cardputerzero-recovery-console.service' "$bsp"
grep -Fq 'device-policy-production.json' "$platform"
grep -Fq 'product:production) IMG_SUFFIX="-cp0-os-production"' \
    "$export_profile"
grep -Fq 'production image contains a human account' "$verifier"
grep -Fq 'production build identity residue remains' "$verifier"
grep -Fq 'production image enables SSH before owner consent' "$verifier"
grep -Fq 'production access unit is not masked' "$verifier"
grep -Fq '.developer_mode_allowed == false' "$verifier"

bash -n "$build" "$bsp" "$platform" "$verifier"
