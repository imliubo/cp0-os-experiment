#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
builtin_apps="$repo_root/config/builtin-apps.tsv"

while IFS=$'\t' read -r example app_id version artifact entrypoint; do
    [[ -n $example && ${example:0:1} != '#' ]] || continue
    example_dir="$repo_root/examples/$example"
    package_dir="$repo_root/target/apps/$app_id/$version"
    source_wasm="$example_dir/target/wasm32-unknown-unknown/release/$artifact"

    cargo build \
        --manifest-path "$example_dir/Cargo.toml" \
        --target wasm32-unknown-unknown \
        --release

    mkdir -p "$package_dir/$(dirname "$entrypoint")"
    install -m 0644 "$source_wasm" "$package_dir/$entrypoint"
    install -m 0644 "$example_dir/app.json" "$package_dir/app.json"
    cargo run -q -p cp0ctl -- manifest validate "$package_dir/app.json"
    jq -e --arg id "$app_id" --arg version "$version" \
        --arg entrypoint "$entrypoint" \
        '.id == $id and .version == $version and .entrypoint == $entrypoint' \
        "$package_dir/app.json" >/dev/null
    test -s "$package_dir/$entrypoint"
    sha256sum "$package_dir/$entrypoint"
done <"$builtin_apps"
