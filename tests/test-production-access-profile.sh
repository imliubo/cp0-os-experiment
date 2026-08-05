#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build="$repo_root/image/build-image.sh"
bsp="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/01-run.sh"
keyboard_patch="$repo_root/image/pi-gen/stage-cardputerzero-os/00-bsp/files/0001-tca8418-flush-synthetic-shift.patch"
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
     .developer_mode_allowed == true and
     .recovery_mode_allowed == false and
     .store_install_allowed == true' \
    "$policy" >/dev/null

grep -Fq 'access_profile=${CP0_ACCESS_PROFILE:-production}' "$build"
grep -Fq 'stage-cardputerzero-os/access-profile' "$build"
grep -Fq 'openssl rand -hex 32' "$build"
grep -Fq 'FIRST_USER_NAME=cp0-build' "$build"
grep -Fq 'device-policy-production.json' "$build"
grep -Fq '/etc/cardputerzero/access-profile' "$bsp"
grep -Fq 'userdel --remove "$FIRST_USER_NAME"' "$bsp"
grep -Fq 'if id -u "$FIRST_USER_NAME" >/dev/null 2>&1; then' "$bsp"
grep -Fq 'build_uid=1000' "$bsp"
grep -Fq 'if getent passwd "$FIRST_USER_NAME" >/dev/null 2>&1; then' "$bsp"
grep -Fq 'temporary product build identity remains' "$bsp"
grep -Fq 'test -e /usr/lib/libnss_extrausers.so.2' "$bsp"
grep -Fq 'locale-gen en_US.UTF-8 zh_CN.UTF-8' "$bsp"
grep -Fq '${STAGE_DIR}/00-bsp/files/0001-tca8418-flush-synthetic-shift.patch' "$bsp"
test "$(grep -c '^+.*input_sync(keypad_data->input);' "$keyboard_patch")" -eq 4
test "$(grep -Ec '^\+[[:space:]]*\{ 0x[[:xdigit:]]{2}, KEY_[A-Z0-9_]+, (true|false) \},$' "$keyboard_patch")" -eq 32
expected_symbol_mappings=(
    '{ 0x44, KEY_1, true },'
    '{ 0x00, KEY_2, true },'
    '{ 0x01, KEY_3, true },'
    '{ 0x02, KEY_4, true },'
    '{ 0x03, KEY_5, true },'
    '{ 0x04, KEY_6, true },'
    '{ 0x05, KEY_7, true },'
    '{ 0x06, KEY_8, true },'
    '{ 0x07, KEY_9, true },'
    '{ 0x08, KEY_0, true },'
    '{ 0x10, KEY_GRAVE, true },'
    '{ 0x11, KEY_GRAVE, false },'
    '{ 0x12, KEY_MINUS, true },'
    '{ 0x13, KEY_MINUS, false },'
    '{ 0x14, KEY_EQUAL, true },'
    '{ 0x15, KEY_EQUAL, false },'
    '{ 0x16, KEY_LEFTBRACE, false },'
    '{ 0x17, KEY_RIGHTBRACE, false },'
    '{ 0x18, KEY_LEFTBRACE, true },'
    '{ 0x19, KEY_RIGHTBRACE, true },'
    '{ 0x20, KEY_SEMICOLON, false },'
    '{ 0x21, KEY_SEMICOLON, true },'
    '{ 0x22, KEY_APOSTROPHE, false },'
    '{ 0x24, KEY_APOSTROPHE, true },'
    '{ 0x25, KEY_COMMA, true },'
    '{ 0x26, KEY_DOT, true },'
    '{ 0x27, KEY_BACKSLASH, false },'
    '{ 0x28, KEY_BACKSLASH, true },'
    '{ 0x35, KEY_COMMA, false },'
    '{ 0x36, KEY_DOT, false },'
    '{ 0x37, KEY_SLASH, false },'
    '{ 0x38, KEY_SLASH, true },'
)
for mapping in "${expected_symbol_mappings[@]}"; do
    grep -Fq "$mapping" "$keyboard_patch"
done
grep -Fq 'tca8418_translate_symbol_key' "$keyboard_patch"
grep -Fq 'KEY_LEFTSHIFT, pressed);' "$keyboard_patch"
grep -Fq 'keypad_data->symbol_shift_count++ == 0' "$keyboard_patch"
grep -Fq 'keypad_data->symbol_shift_count == 0 &&' "$keyboard_patch"
grep -Fq '!keypad_data->asmux_pressed' "$keyboard_patch"
grep -Fq 'input_set_capability(input, EV_KEY, KEY_SLASH)' "$keyboard_patch"
if grep -Eq '^\+[^+].*asmux_(last_release|second_click|oneshot|shift_active|shift_code|locked|unlock_pending|longpress|blink_off)' \
    "$keyboard_patch"; then
    echo "error: V0.6 Shift patch adds latched, locked, or timed behavior" >&2
    exit 1
fi
grep -Fq 'git -C /tmp/cardputerzero-bsp apply --unidiff-zero' "$bsp"
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
if grep -Eq 'maintenance-ssh|cp0-maintenance|hot-update-firstboot' "$bsp"; then
    echo "error: production BSP contains pre-Setup remote access" >&2
    exit 1
fi
grep -Fq 'device-policy-production.json' "$platform"
grep -Fq 'product:production) IMG_SUFFIX="-cp0-os-production"' \
    "$export_profile"
grep -Fq 'production image contains a human account' "$verifier"
grep -Fq 'production build identity residue remains' "$verifier"
grep -Fq 'production image enables SSH before owner consent' "$verifier"
grep -Fq 'production image preauthorizes maintenance access' "$verifier"
grep -Fq 'production image contains pre-Setup remote access' "$verifier"
grep -Fq 'production access unit is not masked' "$verifier"
grep -Fq '.developer_mode_allowed == true' "$verifier"
grep -Fq 'cardputerzero-devd.socket cardputerzero-ssh-access.path' "$platform"
grep -Fq '/usr/libexec/cardputerzero/owner-shell' "$platform"
grep -Fq 'cp0-developer-access' "$repo_root/crates/cp0-provisiond/src/lib.rs"

bash -n "$build" "$bsp" "$platform" "$verifier"
