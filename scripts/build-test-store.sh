#!/usr/bin/env bash
set -euo pipefail

if (($# < 2 || $# > 3)); then
    echo "usage: build-test-store BASE_HTTPS_URL PUBLISHED_UNIX [LIFETIME_SECONDS]" >&2
    exit 2
fi
base_url=${1%/}
published=$2
lifetime=${3:-${CP0_TEST_STORE_LIFETIME_SECONDS:-604800}}
pad_bytes=${CP0_TEST_STORE_PAD_BYTES:-8388608}
if [[ ! $published =~ ^[1-9][0-9]*$ || ! $lifetime =~ ^[1-9][0-9]*$ ||
    ! $pad_bytes =~ ^[1-9][0-9]*$ ]] || ((lifetime < 120 || lifetime > 2678400)) ||
    ((pad_bytes < 65536 || pad_bytes > 16777216)) ||
    ((${#published} > 10)) ||
    ((${#published} == 10 && published > 4102444800)); then
    echo "error: timestamps, lifetime or resume padding are outside acceptance bounds" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/test-store"
developer_secret="$output/developer.key"
developer_public="$output/developer.pub"
store_secret="$output/store.key"
store_public="$output/store.pub"
expires=$((published + lifetime))
sequence_v1=$((published * 10 + 1))
sequence_v2=$((published * 10 + 2))
umask 077
mkdir -p "$output"

ensure_key_pair() {
    local secret=$1 public=$2
    if [[ -e $secret || -e $public ]]; then
        if [[ ! -f $secret || ! -f $public || $(wc -c <"$secret") -ne 32 ||
            $(wc -c <"$public") -ne 32 ]]; then
            echo "error: incomplete or invalid key pair below $output" >&2
            exit 1
        fi
    else
        cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
            key generate "$secret" "$public"
    fi
}

ensure_key_pair "$developer_secret" "$developer_public"
ensure_key_pair "$store_secret" "$store_public"

build_submission() {
    local project=$1 version=$2 label=$3 build_dir unsigned signed submissions reviews sha
    build_dir="$project/target/cardputerzero/dev.cardputerzero.store-test/$version"
    unsigned="$output/store-test-$version.unsigned.capp"
    signed="$output/store-test-$version.capp"
    submissions="$output/submissions-$label"
    reviews="$output/reviews-$label"
    rm -f -- "$unsigned" "$signed"
    rm -rf -- "$build_dir"
    rm -rf -- "$submissions" "$reviews"
    mkdir -p "$submissions" "$reviews"

    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        build "$project"
    mkdir -p "$build_dir/assets"
    head -c "$pad_bytes" /dev/zero >"$build_dir/assets/resume-pad.bin"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        package "$project" "$unsigned"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        sign developer "$unsigned" "$signed" "$developer_secret"
    install -m 0644 "$signed" "$submissions/store-test-$version.capp"
    if command -v sha256sum >/dev/null 2>&1; then
        sha=$(sha256sum "$signed" | awk '{print $1}')
    else
        sha=$(shasum -a 256 "$signed" | awk '{print $1}')
    fi
    jq -n \
        --arg version "$version" \
        --arg sha "$sha" \
        --arg summary "Store acceptance application version $version" \
        --argjson reviewed "$published" \
        '{
          schema_version: 1,
          decision: "approved",
          app_id: "dev.cardputerzero.store-test",
          version: $version,
          submission_sha256: $sha,
          summary: $summary,
          reviewer: "cardputerzero-acceptance",
          reviewed_unix_seconds: $reviewed,
          approved_permissions: [],
          approved_imports: [
            "cp0_display_dimensions",
            "cp0_present_rgb565",
            "cp0_wait_event"
          ]
        }' >"$reviews/dev.cardputerzero.store-test-$version.review.json"
}

build_submission "$repo_root/examples/store-acceptance-v1" 1.0.0 v1
build_submission "$repo_root/examples/store-acceptance-v2" 1.1.0 v2

rm -rf -- "$output/catalog-v1" "$output/catalog-v2"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    store publish "$output/submissions-v1" "$output/reviews-v1" \
    "$output/catalog-v1" "$base_url" "$sequence_v1" "$published" "$expires" \
    "$store_secret"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    store publish "$output/submissions-v2" "$output/reviews-v2" \
    "$output/catalog-v2" "$base_url" "$sequence_v2" "$published" "$expires" \
    "$store_secret"

cmp "$store_public" "$output/catalog-v1/store.pub"
cmp "$store_public" "$output/catalog-v2/store.pub"
for release in v1:1.0.0 v2:1.1.0; do
    catalog="$output/catalog-${release%%:*}"
    version=${release#*:}
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        verify "$catalog/apps/dev.cardputerzero.store-test/$version.capp" \
        "$store_public"
done

if command -v sha256sum >/dev/null 2>&1; then
    store_key_id=$(sha256sum "$store_public" | awk '{print $1}')
else
    store_key_id=$(shasum -a 256 "$store_public" | awk '{print $1}')
fi
jq -n \
    --arg base_url "$base_url" \
    --arg store_key_id "$store_key_id" \
    --argjson published "$published" \
    --argjson expires "$expires" \
    --argjson sequence_v1 "$sequence_v1" \
    --argjson sequence_v2 "$sequence_v2" \
    --argjson package_padding_bytes "$pad_bytes" \
    '{
      schema_version: 1,
      base_url: $base_url,
      catalog_url: ($base_url + "/catalog.json"),
      store_key_id: $store_key_id,
      published_unix_seconds: $published,
      expires_unix_seconds: $expires,
      sequence_v1: $sequence_v1,
      sequence_v2: $sequence_v2,
      package_padding_bytes: $package_padding_bytes
    }' >"$output/acceptance.json"
chmod 0644 "$output/acceptance.json" "$store_public" "$developer_public"

printf 'test Store artifacts: %s\n' "$output"
printf 'catalog URL: %s/catalog.json\n' "$base_url"
printf 'store trust key: %s (%s.pub)\n' "$store_public" "$store_key_id"
printf 'v1 sequence: %s; v2 sequence: %s; expires: %s\n' \
    "$sequence_v1" "$sequence_v2" "$expires"
