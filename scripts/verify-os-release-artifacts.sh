#!/usr/bin/env bash
set -euo pipefail

if (($# != 4)); then
    echo "usage: $0 RELEASE_JSON ROOTFS_IMAGE VERITY_HASH_TREE SIGNED_FIT" >&2
    exit 2
fi

metadata=$1
rootfs=$2
hash_tree=$3
fit=$4
veritysetup=${CP0_VERITYSETUP:-veritysetup}

for path in "$metadata" "$rootfs" "$hash_tree" "$fit"; do
    if [[ ! -f $path || -L $path ]]; then
        echo "error: release artifact is missing, not regular, or symbolic: $path" >&2
        exit 1
    fi
done
command -v jq >/dev/null
if [[ $veritysetup == */* ]]; then
    [[ -x $veritysetup ]]
else
    command -v "$veritysetup" >/dev/null
fi

if ! jq -e '
    . as $release |
    type == "object" and
    (keys | sort) == (["board_id", "data_layout_max", "data_layout_min",
                      "fit", "format", "rootfs", "sequence", "verity",
                      "version"] | sort) and
    .format == "cp0-os-release-v1" and
    .board_id == "cardputerzero-cm0-v0.6" and
    (.version | type == "string" and length >= 5 and length <= 64 and
        test("^(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)\\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\\.[0-9A-Za-z-]+)*)?$")) and
    (.sequence | type == "number" and . >= 1 and . <= 9007199254740991 and floor == .) and
    (.data_layout_min | type == "number" and . >= 1 and . <= 4294967295 and floor == .) and
    (.data_layout_max | type == "number" and . >= $release.data_layout_min and
        . <= 4294967295 and floor == .) and
    (.rootfs | type == "object" and
        (keys | sort) == (["sha256", "size"] | sort) and
        (.sha256 | test("^[0-9a-f]{64}$")) and
        (.size | type == "number" and . >= 1 and . <= 9007199254740991 and floor == .)) and
    (.verity | type == "object" and
        (keys | sort) == (["data_blocks", "hash_tree", "root_hash", "salt"] | sort) and
        (.root_hash | test("^[0-9a-f]{64}$")) and
        (.salt | test("^[0-9a-f]{32,128}$") and (length % 2 == 0)) and
        (.data_blocks | type == "number" and . >= 1 and . <= 2199023255551 and floor == .) and
        (.hash_tree | type == "object" and
            (keys | sort) == (["sha256", "size"] | sort) and
            (.sha256 | test("^[0-9a-f]{64}$")) and
            (.size | type == "number" and . >= 1 and
                . <= 9007199254740991 and floor == .))) and
    (.fit | type == "object" and
        (keys | sort) == (["artifact", "configuration"] | sort) and
        (.configuration == "conf-a" or .configuration == "conf-b") and
        (.artifact | type == "object" and
            (keys | sort) == (["sha256", "size"] | sort) and
            (.sha256 | test("^[0-9a-f]{64}$")) and
            (.size | type == "number" and . >= 1 and
                . <= 9007199254740991 and floor == .)))
' "$metadata" >/dev/null; then
    echo "error: release metadata shape or fixed policy is invalid" >&2
    exit 1
fi

hash_file() {
    if command -v shasum >/dev/null; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

file_size() {
    wc -c <"$1" | tr -d '[:space:]'
}

verify_artifact() {
    path=$1
    expected_hash=$2
    expected_size=$3
    label=$4
    actual_size=$(file_size "$path")
    if [[ $actual_size != "$expected_size" ]]; then
        echo "error: $label size does not match authenticated metadata" >&2
        exit 1
    fi
    actual_hash=$(hash_file "$path")
    if [[ $actual_hash != "$expected_hash" ]]; then
        echo "error: $label SHA-256 does not match authenticated metadata" >&2
        exit 1
    fi
}

rootfs_hash=$(jq -r '.rootfs.sha256' "$metadata")
rootfs_size=$(jq -r '.rootfs.size' "$metadata")
hash_tree_hash=$(jq -r '.verity.hash_tree.sha256' "$metadata")
hash_tree_size=$(jq -r '.verity.hash_tree.size' "$metadata")
fit_hash=$(jq -r '.fit.artifact.sha256' "$metadata")
fit_size=$(jq -r '.fit.artifact.size' "$metadata")
data_blocks=$(jq -r '.verity.data_blocks' "$metadata")
root_hash=$(jq -r '.verity.root_hash' "$metadata")
salt=$(jq -r '.verity.salt' "$metadata")

if ((data_blocks > 0 && data_blocks > 9223372036854775807 / 4096)); then
    echo "error: verity data block count overflows the verifier" >&2
    exit 1
fi
if ((data_blocks * 4096 != rootfs_size)); then
    echo "error: rootfs size is not exactly the declared verity data size" >&2
    exit 1
fi

verify_artifact "$rootfs" "$rootfs_hash" "$rootfs_size" rootfs
verify_artifact "$hash_tree" "$hash_tree_hash" "$hash_tree_size" "verity hash tree"
verify_artifact "$fit" "$fit_hash" "$fit_size" "signed FIT"

"$veritysetup" \
    --hash=sha256 \
    --data-block-size=4096 \
    --hash-block-size=4096 \
    --data-blocks="$data_blocks" \
    --salt="$salt" \
    verify "$rootfs" "$hash_tree" "$root_hash"

echo "PASS OS release artifacts and dm-verity tree"
