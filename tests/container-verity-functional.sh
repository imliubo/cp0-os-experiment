#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
verifier="$repo_root/scripts/verify-os-release-artifacts.sh"
command -v jq >/dev/null
command -v veritysetup >/dev/null

test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
test_root=$(mktemp -d "$test_parent/cp0-verity-functional.XXXXXX")
case "$test_root" in
    "$test_parent"/cp0-verity-functional.*) ;;
    *)
        echo "error: unsafe verity functional test directory" >&2
        exit 1
        ;;
esac
trap 'rm -rf -- "$test_root"' EXIT

rootfs="$test_root/rootfs.img"
hash_tree="$test_root/rootfs.verity"
fit="$test_root/slot.itb"
metadata="$test_root/release.json"
salt=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef

dd if=/dev/zero of="$rootfs" bs=4096 count=64 status=none
printf 'signed-fit-functional-fixture\n' >"$fit"
format_output=$(veritysetup \
    --hash=sha256 \
    --data-block-size=4096 \
    --hash-block-size=4096 \
    --salt="$salt" \
    format "$rootfs" "$hash_tree")
root_hash=$(sed -n \
    's/^[[:space:]]*Root hash:[[:space:]]*\([0-9a-f]*\)[[:space:]]*$/\1/p' \
    <<<"$format_output")
if [[ ! $root_hash =~ ^[0-9a-f]{64}$ ]]; then
    echo "error: veritysetup did not produce a SHA-256 root hash" >&2
    exit 1
fi

hash_file() {
    if command -v shasum >/dev/null; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

write_metadata() {
    jq -n \
        --arg rootfs_hash "$(hash_file "$rootfs")" \
        --argjson rootfs_size "$(wc -c <"$rootfs")" \
        --arg tree_hash "$(hash_file "$hash_tree")" \
        --argjson tree_size "$(wc -c <"$hash_tree")" \
        --arg fit_hash "$(hash_file "$fit")" \
        --argjson fit_size "$(wc -c <"$fit")" \
        --arg root_hash "$root_hash" \
        --arg salt "$salt" \
        '{
            format: "cp0-os-release-v1",
            board_id: "cardputerzero-cm0-v0.6",
            version: "1.0.0",
            sequence: 2,
            data_layout_min: 1,
            data_layout_max: 1,
            rootfs: {sha256: $rootfs_hash, size: $rootfs_size},
            verity: {
                root_hash: $root_hash,
                salt: $salt,
                data_blocks: 64,
                hash_tree: {sha256: $tree_hash, size: $tree_size}
            },
            fit: {
                artifact: {sha256: $fit_hash, size: $fit_size},
                configuration: "conf-b"
            }
        }' >"$metadata"
}

write_metadata
CP0_VERITYSETUP=veritysetup \
    "$verifier" "$metadata" "$rootfs" "$hash_tree" "$fit" >/dev/null

printf '\001' | dd of="$hash_tree" bs=1 seek=4096 count=1 conv=notrunc status=none
write_metadata
if CP0_VERITYSETUP=veritysetup \
    "$verifier" "$metadata" "$rootfs" "$hash_tree" "$fit" >/dev/null 2>&1; then
    echo "error: corrupt authenticated verity tree passed cryptographic verification" >&2
    exit 1
fi

echo "PASS real dm-verity artifact verification"
