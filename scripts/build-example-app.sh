#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
example_dir="$repo_root/examples/hello-card"
package_dir="$repo_root/target/apps/dev.cardputerzero.hello/0.1.0"

cargo build \
    --manifest-path "$example_dir/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release

mkdir -p "$package_dir/bin"
install -m 0644 "$example_dir/target/wasm32-unknown-unknown/release/hello_card.wasm" \
    "$package_dir/bin/hello-card.wasm"
install -m 0644 "$example_dir/app.json" "$package_dir/app.json"

cargo run -q -p cp0ctl -- manifest validate "$package_dir/app.json"
test -s "$package_dir/bin/hello-card.wasm"
sha256sum "$package_dir/bin/hello-card.wasm"
