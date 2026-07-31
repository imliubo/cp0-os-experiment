#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
wrapper="$repo_root/scripts/device-recovery-data.sh"
build="$repo_root/image/build-image.sh"
stage="$repo_root/image/pi-gen/stage-cardputerzero-os/02-app-platform/01-run.sh"
builder="$repo_root/scripts/build-appd.sh"
verifier="$repo_root/tests/test-built-rootfs-profile.sh"
library="$repo_root/crates/cp0-recovery/src/lib.rs"

bash -n "$wrapper" "$build" "$stage" "$builder" "$verifier"
grep -q 'CP0BKUP\\0' "$library"
grep -q 'MAX_PAYLOAD_BYTES' "$library"
grep -q 'O_NOFOLLOW' "$library"
grep -q 'hard-linked file is not supported' "$library"
grep -q 'world-writable entry is not supported' "$library"
grep -q 'entry path is outside the persistent allowlist' "$library"
grep -q 'restore target is not empty' "$library"

grep -q '^mount_root=/run/cardputerzero-recovery-data$' "$wrapper"
grep -q 'cp0.overlay_root=volatile' "$wrapper"
grep -q 'PARTN.*!= 3' "$wrapper"
grep -q 'LABEL.*cp0-data' "$wrapper"
grep -q 'TYPE.*ext4' "$wrapper"
grep -q 'cp0-data must be unmounted before recovery' "$wrapper"
grep -q 'refusing to operate on the active root filesystem' "$wrapper"
grep -q 'backup output must be on a separately mounted filesystem' "$wrapper"
grep -q 'RESTORE-CP0-DATA' "$wrapper"
grep -q 'RESET-CP0-DATA' "$wrapper"
grep -q 'factory-reset requires the product lower-root maintenance profile' "$wrapper"
grep -q '/usr/bin/cp0-recovery verify' "$wrapper"
grep -q 'mkfs.ext4 -F -L cp0-data' "$wrapper"
grep -q 'e2fsck -pf' "$wrapper"

verify_line=$(grep -n 'verify_output=.*cp0-recovery verify' "$wrapper" | cut -d: -f1)
format_line=$(grep -n 'mkfs.ext4 -F -L cp0-data' "$wrapper" | cut -d: -f1)
if [[ -z $verify_line || -z $format_line ]] || ((verify_line >= format_line)); then
    echo "error: recovery restore does not verify before formatting" >&2
    exit 1
fi
if grep -Eq '(^|[[:space:]])(dd|wipefs)([[:space:]]|$)' "$wrapper"; then
    echo "error: recovery wrapper contains an unreviewed raw destructive command" >&2
    exit 1
fi

grep -q 'release/cp0-recovery' "$build"
grep -q 'device-recovery-data.sh' "$build"
grep -q -- '-p cp0-recovery' "$builder"
grep -q 'usr/bin/cp0-recovery' "$stage"
grep -q 'device-recovery-data' "$stage"
grep -q 'factory-data-v1.cp0backup' "$stage"
grep -q 'printf.*product.*image-profile' "$stage"
grep -q ': >"\$factory_root/machine-id"' "$stage"
grep -q ': >"\$factory_root/random-seed"' "$stage"
grep -q 'usr/bin/cp0-recovery' "$verifier"
grep -q 'regenerate_ssh_host_keys.service' "$verifier"
grep -q 'cp0-recovery verify.*factory_bundle' "$verifier"
grep -q 'profile=product' "$verifier"
grep -q 'recovery image contains an incomplete product factory seed' "$verifier"
