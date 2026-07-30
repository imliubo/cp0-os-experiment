#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pi_gen_dir=${PI_GEN_DIR:-}
password=${CP0_FIRST_USER_PASSWORD:-}

if [[ $(uname -s) != Linux ]]; then
    echo "error: pi-gen must run on Linux or in a Linux VM/container" >&2
    exit 1
fi
if [[ -z "$pi_gen_dir" || ! -x "$pi_gen_dir/build.sh" ]]; then
    echo "error: PI_GEN_DIR must point to a pi-gen checkout" >&2
    exit 1
fi
if [[ -z "$password" ]]; then
    echo "error: CP0_FIRST_USER_PASSWORD is required for the development image" >&2
    exit 1
fi

config_file=$(mktemp "${TMPDIR:-/tmp}/cp0-pigen-config.XXXXXX")
cleanup() {
    rm -f "$config_file"
}
trap cleanup EXIT

cp "$repo_root/image/pi-gen/config.example" "$config_file"
printf 'FIRST_USER_PASS=%q\n' "$password" >>"$config_file"

if [[ -n ${CP0_SSH_PUBLIC_KEY:-} ]]; then
    printf 'PUBKEY_SSH_FIRST_USER=%q\n' "$CP0_SSH_PUBLIC_KEY" >>"$config_file"
    printf 'PUBKEY_ONLY_SSH=1\n' >>"$config_file"
fi

printf 'STAGE_LIST=%q\n' \
    "$pi_gen_dir/stage0 $pi_gen_dir/stage1 $pi_gen_dir/stage2 $repo_root/image/pi-gen/stage-cardputerzero-os" \
    >>"$config_file"

echo "Building CardputerZero OS with pi-gen at $pi_gen_dir"
"$pi_gen_dir/build.sh" -c "$config_file"
