#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
project="$repo_root/examples/keyboard-diagnostics"
output="$repo_root/target/keyboard-diagnostics"
unsigned="$output/keyboard-diagnostics.unsigned.capp"
signed="$output/keyboard-diagnostics.capp"
secret=${CP0_DIAGNOSTICS_DEVELOPER_KEY:-$repo_root/target/device-capability-acceptance/developer.key}

mkdir -p "$output"
cargo fmt --manifest-path "$project/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$project/Cargo.toml"
rm -f -- "$unsigned" "$signed"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    package "$project" "$unsigned"

if [[ -f $secret && $(wc -c <"$secret") -eq 32 ]]; then
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        sign developer "$unsigned" "$signed" "$secret"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        verify "$signed"
    printf 'signed keyboard diagnostics: %s\n' "$signed"
else
    printf 'unsigned keyboard diagnostics: %s\n' "$unsigned"
    printf 'set CP0_DIAGNOSTICS_DEVELOPER_KEY to a trusted 32-byte key to sign it\n'
fi
