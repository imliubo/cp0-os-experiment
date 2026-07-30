#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work_dir="$repo_root/target/test-tmp/system-shell-ui.$$"
mkdir -p "$work_dir"
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM
snapshot_dir="$work_dir/snapshots"
mkdir -p "$snapshot_dir"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/ui.c" \
    "$repo_root/tests/system-shell-ui.c" \
    -o "$work_dir/system-shell-ui-test"

"$work_dir/system-shell-ui-test" "$snapshot_dir"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/tests/system-shell-json.c" \
    -o "$work_dir/system-shell-json-test"
"$work_dir/system-shell-json-test"

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(cd "$snapshot_dir" && \
        sha256sum apps.ppm home.ppm permission.ppm power.ppm tasks.ppm)
else
    actual=$(cd "$snapshot_dir" && \
        shasum -a 256 apps.ppm home.ppm permission.ppm power.ppm tasks.ppm)
fi

expected='c6af580b7e821c08eb0fa72edff1ef2fdeaaeb36d3ee49597e5b5b359ff0cf71  apps.ppm
4e5d5f4cef0235b44e87e9f40e00aef57ebf21f35bc7b6b897cc674cc04d8d81  home.ppm
3c9f90a8bcc0c5d5ffaad46d31748dd831fface44e7615083e1e8357b63256a6  permission.ppm
a6e5f954c77d1512c6abdd25d2b28a836983423a3ceb0990d014282915eff406  power.ppm
b382c359864c04060e4676c13c50e9578d241f132009a2840a3a9ed8324cfae2  tasks.ppm'

if [ "$actual" != "$expected" ]; then
    echo "System Shell screenshot regression:" >&2
    echo "$actual" >&2
    exit 1
fi
