#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "usage: doctor.sh [DEVKIT_ROOT] [rust|c|all]" >&2
    exit 2
}

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=${1:-${CP0_DEVKIT_ROOT:-$script_dir/../../..}}
language=${2:-rust}
case "$language" in
    rust | c | all) ;;
    *) usage ;;
esac
if [[ ! -d $root ]]; then
    echo "error: DevKit root does not exist: $root" >&2
    exit 1
fi
root=$(cd "$root" && pwd -P)

missing=0
require_file() {
    if [[ ! -f $1 ]]; then
        echo "MISSING file $1" >&2
        missing=1
    fi
}
require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        echo "MISSING command $1" >&2
        missing=1
    fi
}

require_file "$root/sdk/rust/Cargo.toml"
require_file "$root/sdk/rust/src/media.rs"
require_file "$root/sdk/rust/src/lifecycle.rs"
require_file "$root/sdk/c/include/cardputerzero.h"
require_file "$root/sdk/wit/cardputerzero-sdk.wit"
require_file "$root/simulator/cp0-simulator.mjs"
require_file "$root/schemas/store-listing-v1.schema.json"
require_file "$root/devkit/toolchain.toml"
require_command node

if [[ $language == rust || $language == all ]]; then
    require_command cargo
    require_command rustc
    require_command rustup
fi
if [[ $language == c || $language == all ]]; then
    require_command emcc
    require_command em++
fi
if ((missing != 0)); then
    exit 1
fi

node_version=$(node --version)
node_major=${node_version#v}
node_major=${node_major%%.*}
if [[ ! $node_major =~ ^[0-9]+$ ]] || ((node_major < 20)); then
    echo "error: Node 20 or newer is required; found $node_version" >&2
    exit 1
fi

if [[ $language == rust || $language == all ]]; then
    pinned_rust=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
        "$root/devkit/toolchain.toml")
    if [[ -z $pinned_rust ]] ||
        ! rustup run "$pinned_rust" rustc --version >/dev/null 2>&1; then
        echo "error: pinned Rust $pinned_rust is not installed" >&2
        exit 1
    fi
    rust_version=$(rustup run "$pinned_rust" rustc --version | awk '{ print $2 }')
    if ! rustup target list --toolchain "$pinned_rust" --installed \
        | grep -qx wasm32-unknown-unknown; then
        echo "error: Rust target wasm32-unknown-unknown is not installed for $pinned_rust" >&2
        exit 1
    fi
fi

printf 'PASS CardputerZero DevKit root=%s language=%s node=%s' \
    "$root" "$language" "$node_version"
if [[ $language == rust || $language == all ]]; then
    printf ' rust=%s' "$rust_version"
fi
if [[ $language == c || $language == all ]]; then
    printf ' emcc=%s' "$(emcc --version | head -n 1)"
fi
printf '\n'
