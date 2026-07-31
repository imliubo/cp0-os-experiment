#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verifier="$repo_root/scripts/verify-os-release-artifacts.sh"
schema="$repo_root/schemas/os-release-v1.schema.json"
library="$repo_root/crates/cp0-os-update/src/lib.rs"

bash -n "$verifier"
jq empty "$schema"
grep -q '^pub const MAX_BOOT_ATTEMPTS: u8 = 3;' "$library"
grep -q 'Callers must not treat successful JSON validation as signature verification' \
    "$library"
grep -q 'persisted before transferring' "$library"
grep -q 'booted slot and sequence do not match the pending release' "$library"
grep -q 'logically_impossible_boot_states_fail_with_valid_checksums' "$library"
grep -q 'one_hundred_interrupted_updates_always_retain_a_bootable_slot' "$library"
grep -q -- '--data-block-size=4096' "$verifier"
grep -q -- '--hash-block-size=4096' "$verifier"

test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
test_root=$(mktemp -d "$test_parent/cp0-os-update-test.XXXXXX")
case "$test_root" in
    "$test_parent"/cp0-os-update-test.*) ;;
    *)
        echo "error: unsafe OS update test directory" >&2
        exit 1
        ;;
esac
trap 'rm -rf -- "$test_root"' EXIT

rootfs="$test_root/rootfs.img"
hash_tree="$test_root/rootfs.verity"
fit="$test_root/slot.itb"
metadata="$test_root/release.json"
mock="$test_root/veritysetup"
args="$test_root/veritysetup.args"

dd if=/dev/zero of="$rootfs" bs=4096 count=2 status=none
printf 'verity-tree-fixture\n' >"$hash_tree"
printf 'signed-fit-fixture\n' >"$fit"

hash_file() {
    if command -v shasum >/dev/null; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

jq -n \
    --arg rootfs_hash "$(hash_file "$rootfs")" \
    --argjson rootfs_size "$(wc -c <"$rootfs")" \
    --arg tree_hash "$(hash_file "$hash_tree")" \
    --argjson tree_size "$(wc -c <"$hash_tree")" \
    --arg fit_hash "$(hash_file "$fit")" \
    --argjson fit_size "$(wc -c <"$fit")" \
    '{
        format: "cp0-os-release-v1",
        board_id: "cardputerzero-cm0-v0.6",
        version: "1.0.0",
        sequence: 2,
        data_layout_min: 1,
        data_layout_max: 1,
        rootfs: {sha256: $rootfs_hash, size: $rootfs_size},
        verity: {
            root_hash: ("a" * 64),
            salt: ("b" * 64),
            data_blocks: 2,
            hash_tree: {sha256: $tree_hash, size: $tree_size}
        },
        fit: {
            artifact: {sha256: $fit_hash, size: $fit_size},
            configuration: "conf-b"
        }
    }' >"$metadata"

cat >"$mock" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" >"$CP0_VERITYSETUP_ARGS"
MOCK
chmod 0755 "$mock"

CP0_VERITYSETUP="$mock" CP0_VERITYSETUP_ARGS="$args" \
    "$verifier" "$metadata" "$rootfs" "$hash_tree" "$fit" >/dev/null
grep -qx -- '--data-blocks=2' "$args"
grep -qx -- '--salt=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' \
    "$args"
grep -qx 'verify' "$args"
grep -Fxq "$rootfs" "$args"
grep -Fxq "$hash_tree" "$args"

printf 'tamper' >>"$rootfs"
if CP0_VERITYSETUP="$mock" CP0_VERITYSETUP_ARGS="$args" \
    "$verifier" "$metadata" "$rootfs" "$hash_tree" "$fit" >/dev/null 2>&1; then
    echo "error: tampered rootfs passed release artifact verification" >&2
    exit 1
fi

dd if=/dev/zero of="$rootfs" bs=4096 count=2 status=none
jq '.unexpected = true' "$metadata" >"$metadata.invalid"
if CP0_VERITYSETUP="$mock" CP0_VERITYSETUP_ARGS="$args" \
    "$verifier" "$metadata.invalid" "$rootfs" "$hash_tree" "$fit" \
    >/dev/null 2>&1; then
    echo "error: release metadata with an unknown field passed verification" >&2
    exit 1
fi

echo "PASS OS update policy profile"
