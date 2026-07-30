#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
temp_dir="$repo_root/target/test-tmp/cp0-dtb-test.$$"
mkdir -p "$temp_dir"
cleanup() {
    rm -f "$temp_dir/input.dtb" "$temp_dir/output.dtb" \
        "$temp_dir/output-again.dtb"
    rmdir "$temp_dir"
}
trap cleanup EXIT

dtc -I dts -O dtb \
    -o "$temp_dir/input.dtb" \
    "$repo_root/tests/fixtures/cm0-bootargs.dts"
"$repo_root/scripts/patch-cm0-dtb.sh" \
    "$temp_dir/input.dtb" \
    "$temp_dir/output.dtb" >/dev/null

result=$(fdtget -t s "$temp_dir/output.dtb" /chosen bootargs)
expected='coherent_pool=1M snd_bcm2835.enable_hdmi=0'
if [[ "$result" != "$expected" ]]; then
    echo "unexpected patched bootargs: $result" >&2
    exit 1
fi

"$repo_root/scripts/patch-cm0-dtb.sh" \
    "$temp_dir/output.dtb" \
    "$temp_dir/output-again.dtb" >/dev/null
result=$(fdtget -t s "$temp_dir/output-again.dtb" /chosen bootargs)
if [[ "$result" != "$expected" ]]; then
    echo "unexpected bootargs after second patch: $result" >&2
    exit 1
fi

echo "DTB bootargs patch test passed"
