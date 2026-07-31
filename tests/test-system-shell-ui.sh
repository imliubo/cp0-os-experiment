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

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_STORE_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/store_client.c" \
    "$repo_root/tests/system-shell-store-client.c" \
    -o "$work_dir/system-shell-store-client-test"
"$work_dir/system-shell-store-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_SYSTEM_INFO_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/system_info.c" \
    "$repo_root/tests/system-shell-system-info.c" \
    -o "$work_dir/system-shell-system-info-test"
"$work_dir/system-shell-system-info-test"

if command -v sha256sum >/dev/null 2>&1; then
    actual=$(cd "$snapshot_dir" && \
        sha256sum app-detail.ppm apps-empty.ppm apps.ppm device-resources.ppm device-unavailable.ppm device.ppm document.ppm home.ppm network-detail.ppm network-offline.ppm network.ppm notification.ppm permission.ppm power.ppm settings-confirm.ppm settings-policy.ppm settings.ppm store-detail.ppm store.ppm tasks.ppm)
else
    actual=$(cd "$snapshot_dir" && \
        shasum -a 256 app-detail.ppm apps-empty.ppm apps.ppm device-resources.ppm device-unavailable.ppm device.ppm document.ppm home.ppm network-detail.ppm network-offline.ppm network.ppm notification.ppm permission.ppm power.ppm settings-confirm.ppm settings-policy.ppm settings.ppm store-detail.ppm store.ppm tasks.ppm)
fi

expected='9906dd2e7aa07a583b832308205635d69110dc83a1233ee7988d1dad7b0b3445  app-detail.ppm
e690bfa55247afebf1858fbf0151e805f7446ed1d5ae990f5f0680f8b4b03e5a  apps-empty.ppm
895d13c55341090c408fa658ef89aca771dc4ae32a8d741d8cefe2194cb71e70  apps.ppm
00a3a7a87c813b3abfbea95e38d242ae5d0f749e435275ac9ee6f72c72d81a81  device-resources.ppm
8fb8a471d993042126e54269fb44e9d5b52805e47310f72ebf408c60788612f4  device-unavailable.ppm
301b8e07610fc2de5efe5284f66c5ddd1dbbb9d97ef83db7bdd4b9f022b92fd0  device.ppm
b30227204599548f1de899e863e80589155694b5f3240b8c598c1396a3d21c76  document.ppm
40258e54baae3a7a41765f2e95dc81f051765f0e1f89b4f654fd7152b0ebc3e5  home.ppm
d0f6c4fb530e418af657af2cf5297d52cbe42b612eb1005537fc855c63be6dae  network-detail.ppm
2962d9d9626435eb9b4dbf41a8cd80cab8c213e73bb81c3c9142ee7e411d6d5d  network-offline.ppm
b99153076a10fd8da5eb18448a195447a4b4dd65f1422403cc3a365bc889f738  network.ppm
8fb4e226637acb0f85027f430d4f7ff94d7ed33764a77ceac73ba0411bc2d943  notification.ppm
3c9f90a8bcc0c5d5ffaad46d31748dd831fface44e7615083e1e8357b63256a6  permission.ppm
a6e5f954c77d1512c6abdd25d2b28a836983423a3ceb0990d014282915eff406  power.ppm
b3dde2798ce27b9cd19af203d27401106e94e2351cc10773b391375683a219b5  settings-confirm.ppm
db332ec7ed9804923ae103eb1b6fd4442a79740335ea1127f5359a70d63ca5a4  settings-policy.ppm
e7012c258b6838bc07e19c901028ddd6fc9122a5a7830b017c1d94631fc9c149  settings.ppm
e5df489f49f025c4c7c2fdee52231cad8501f94a2f65493096b7a0394bddeca3  store-detail.ppm
80a18453b84e8c7e565410ac1c975bad457f1bffe030d3b5f7cc304b7f933324  store.ppm
3b71571633eb8db0f5bb38373a5fa98dc497d17a425587aec58d09966a0fc173  tasks.ppm'

if [ "$actual" != "$expected" ]; then
    echo "System Shell screenshot regression:" >&2
    echo "$actual" >&2
    exit 1
fi
