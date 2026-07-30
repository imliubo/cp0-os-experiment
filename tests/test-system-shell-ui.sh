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

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_APPD_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/appd_client.c" \
    "$repo_root/tests/system-shell-appd-client.c" \
    -o "$work_dir/system-shell-appd-client-test"
"$work_dir/system-shell-appd-client-test"

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(cd "$snapshot_dir" && \
        sha256sum apps.ppm document.ppm home.ppm notification.ppm permission.ppm power.ppm tasks.ppm)
else
    actual=$(cd "$snapshot_dir" && \
        shasum -a 256 apps.ppm document.ppm home.ppm notification.ppm permission.ppm power.ppm tasks.ppm)
fi

expected='895d13c55341090c408fa658ef89aca771dc4ae32a8d741d8cefe2194cb71e70  apps.ppm
b30227204599548f1de899e863e80589155694b5f3240b8c598c1396a3d21c76  document.ppm
4e5d5f4cef0235b44e87e9f40e00aef57ebf21f35bc7b6b897cc674cc04d8d81  home.ppm
8fb4e226637acb0f85027f430d4f7ff94d7ed33764a77ceac73ba0411bc2d943  notification.ppm
3c9f90a8bcc0c5d5ffaad46d31748dd831fface44e7615083e1e8357b63256a6  permission.ppm
a6e5f954c77d1512c6abdd25d2b28a836983423a3ceb0990d014282915eff406  power.ppm
3b71571633eb8db0f5bb38373a5fa98dc497d17a425587aec58d09966a0fc173  tasks.ppm'

if [ "$actual" != "$expected" ]; then
    echo "System Shell screenshot regression:" >&2
    echo "$actual" >&2
    exit 1
fi
