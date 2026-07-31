#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/device-capability-acceptance"
secret="$output/developer.key"
public="$output/developer.pub"
mkdir -p "$output"

if [[ -e $secret || -e $public ]]; then
    if [[ ! -f $secret || ! -f $public || $(wc -c <"$secret") -ne 32 ||
        $(wc -c <"$public") -ne 32 ]]; then
        echo "error: acceptance developer key pair is incomplete or invalid" >&2
        exit 1
    fi
else
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        key generate "$secret" "$public"
fi

build_and_sign() {
    local project=$1 name=$2 unsigned signed
    unsigned="$output/$name.unsigned.capp"
    signed="$output/$name.capp"
    rm -f -- "$unsigned" "$signed"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        package "$project" "$unsigned"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        sign developer "$unsigned" "$signed" "$secret"
    cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
        verify "$signed"
}

build_and_sign "$repo_root/examples/device-capability-probe" \
    device-capability-probe
build_and_sign "$repo_root/examples/storage-isolation-probe" \
    storage-isolation-probe

if command -v sha256sum >/dev/null 2>&1; then
    key_id=$(sha256sum "$public" | awk '{print $1}')
else
    key_id=$(shasum -a 256 "$public" | awk '{print $1}')
fi
printf 'acceptance artifacts: %s\n' "$output"
printf 'developer public key ID: %s\n' "$key_id"
printf 'install the public key as: /etc/cardputerzero/trust/developers/%s.pub\n' \
    "$key_id"
