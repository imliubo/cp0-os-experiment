#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
build_script="$repo_root/scripts/build-test-store.sh"
device_script="$repo_root/scripts/device-store-acceptance.sh"
output="$repo_root/target/test-store"
base_url=https://store.example.com/cardputerzero-acceptance
published=1800000000
lifetime=600
sequence_v1=18000000001
sequence_v2=18000000002

bash -n "$build_script" "$device_script"
cargo fmt --manifest-path "$repo_root/examples/store-acceptance-v1/Cargo.toml" -- --check
cargo fmt --manifest-path "$repo_root/examples/store-acceptance-v2/Cargo.toml" -- --check

CP0_TEST_STORE_PAD_BYTES=65536 \
    "$build_script" "$base_url" "$published" "$lifetime"

jq -e '
    .schema_version == 1 and
    .base_url == "https://store.example.com/cardputerzero-acceptance" and
    .catalog_url == "https://store.example.com/cardputerzero-acceptance/catalog.json" and
    .published_unix_seconds == 1800000000 and
    .expires_unix_seconds == 1800000600 and
    .sequence_v1 == 18000000001 and
    .sequence_v2 == 18000000002 and
    .package_padding_bytes == 65536
' "$output/acceptance.json" >/dev/null

for release in v1:1.0.0:18000000001 v2:1.1.0:18000000002; do
    label=${release%%:*}
    remainder=${release#*:}
    version=${remainder%%:*}
    sequence=${remainder#*:}
    catalog="$output/catalog-$label/catalog.json"
    package="$output/catalog-$label/apps/dev.cardputerzero.store-test/$version.capp"
    jq -e \
        --arg version "$version" \
        --arg url "$base_url/apps/dev.cardputerzero.store-test/$version.capp" \
        --argjson sequence "$sequence" '
        .catalog.schema_version == 1 and
        .catalog.sequence == $sequence and
        (.catalog.apps | length) == 1 and
        .catalog.apps[0].app_id == "dev.cardputerzero.store-test" and
        .catalog.apps[0].version == $version and
        .catalog.apps[0].package_url == $url and
        .catalog.apps[0].package_bytes > 65536 and
        .catalog.apps[0].permissions == [] and
        (.key_id | length) == 64 and
        (.signature | length) == 128
    ' "$catalog" >/dev/null
    test -f "$package"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        verify "$package" "$output/store.pub" >/dev/null
    cmp "$output/store.pub" "$output/catalog-$label/store.pub"
done

for release in v1:1.0.0 v2:1.1.0; do
    label=${release%%:*}
    version=${release#*:}
    jq -e '
        .decision == "approved" and
        .app_id == "dev.cardputerzero.store-test" and
        .approved_permissions == [] and
        .approved_imports == [
          "cp0_display_dimensions",
          "cp0_present_rgb565",
          "cp0_wait_event"
        ]
    ' "$output/reviews-$label/dev.cardputerzero.store-test-$version.review.json" \
        >/dev/null
done

first_digest=$(shasum -a 256 \
    "$output/catalog-v1/catalog.json" \
    "$output/catalog-v1/apps/dev.cardputerzero.store-test/1.0.0.capp" \
    "$output/catalog-v2/catalog.json" \
    "$output/catalog-v2/apps/dev.cardputerzero.store-test/1.1.0.capp" |
    shasum -a 256 | awk '{print $1}')
CP0_TEST_STORE_PAD_BYTES=65536 \
    "$build_script" "$base_url" "$published" "$lifetime" >/dev/null
second_digest=$(shasum -a 256 \
    "$output/catalog-v1/catalog.json" \
    "$output/catalog-v1/apps/dev.cardputerzero.store-test/1.0.0.capp" \
    "$output/catalog-v2/catalog.json" \
    "$output/catalog-v2/apps/dev.cardputerzero.store-test/1.1.0.capp" |
    shasum -a 256 | awk '{print $1}')
test "$first_digest" = "$second_digest"

grep -q 'cardputerzero-stability-acceptance.service' "$device_script"
grep -q 'systemctl kill --kill-whom=main --signal=KILL' "$device_script"
grep -q 'cardputerzero-stored.service' "$device_script"
grep -q 'partial_size < package_bytes' "$device_script"
grep -q 'refresh-v1 | refresh-v2 | offline-v2 | stale-v2' "$device_script"
grep -q 'resume-v1 | upgrade-v2' "$device_script"
grep -q 'resuming package download from byte' \
    "$repo_root/crates/cp0-stored/src/lib.rs"
if grep -q 'CP0_STORE_SOCKET' "$device_script"; then
    echo "error: Store acceptance bypasses the production Store socket" >&2
    exit 1
fi
