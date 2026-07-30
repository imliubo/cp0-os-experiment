#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo "usage: $0 <bcm2710-rpi-cm0.dtb> [output.dtb]" >&2
    exit 2
fi
if ! command -v fdtget >/dev/null || ! command -v fdtput >/dev/null; then
    echo "error: device-tree-compiler tools fdtget and fdtput are required" >&2
    exit 1
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
input=$1
output=${2:-"$repo_root/target/bsp/bcm2710-rpi-cm0.dtb"}
bootargs=$(fdtget -t s "$input" /chosen bootargs)

filtered=()
for token in $bootargs; do
    [[ "$token" == cgroup_disable=memory ]] || filtered+=("$token")
done

mkdir -p "$(dirname "$output")"
cp "$input" "$output"
if [[ "$bootargs" != "${filtered[*]}" ]]; then
    fdtput -t s "$output" /chosen bootargs "${filtered[*]}"
fi

result=$(fdtget -t s "$output" /chosen bootargs)
if [[ " $result " == *" cgroup_disable=memory "* ]]; then
    echo "error: failed to remove cgroup_disable=memory" >&2
    exit 1
fi

echo "$output"
