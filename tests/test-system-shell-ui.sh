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
    $(pkg-config --cflags --libs libpng) \
    -o "$work_dir/system-shell-store-client-test"
"$work_dir/system-shell-store-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_SYSTEM_INFO_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/system_info.c" \
    "$repo_root/tests/system-shell-system-info.c" \
    -o "$work_dir/system-shell-system-info-test"
"$work_dir/system-shell-system-info-test"

snapshot_files='app-actions.ppm app-overview.ppm app-permissions.ppm app-storage.ppm app-uninstall.ppm apps-empty.ppm apps.ppm device-diagnostics.ppm device-power.ppm device-resources.ppm device-unavailable.ppm device.ppm document.ppm home.ppm network-detail.ppm network-offline.ppm network.ppm notification.ppm permission.ppm power.ppm settings-apps-privacy.ppm settings-auto-update.ppm settings-camera.ppm settings-confirm.ppm settings-connectivity.ppm settings-display.ppm settings-power.ppm settings-security.ppm settings-sound.ppm settings-system.ppm settings.ppm store-background-progress.ppm store-description.ppm store-detail.ppm store-failed.ppm store-install-confirm.ppm store-install-storage.ppm store-permissions.ppm store-release-notes.ppm store-screenshot.ppm store-search-empty.ppm store-search-max.ppm store-search-none.ppm store-search-recent.ppm store-search.ppm store-updates.ppm store.ppm system-brightness.ppm system-help.ppm theme-high-contrast.ppm theme-light.ppm tasks.ppm'
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(cd "$snapshot_dir" && sha256sum $snapshot_files)
else
    actual=$(cd "$snapshot_dir" && shasum -a 256 $snapshot_files)
fi

expected='b252a095d38eac93567ff1d5c8d2fa8edee762151cc72b442616ae59a60eeb3f  app-actions.ppm
e96b09556b90a71d7102472be11a53abba0e916b8ce513defe2f7ca7cbb9dfb1  app-overview.ppm
e5854e36cef4114241b0ba61cd4a7c4192e023726388c63357f31d27f982ff5f  app-permissions.ppm
bc829a4605fdee296b28bf5949812daa3a39b455929ba0201987d856d5d1a3a2  app-storage.ppm
2b815dc43b3ea4e80cf2211fc050c8d5350f0aa654cce5d7164214e31000f9ce  app-uninstall.ppm
e690bfa55247afebf1858fbf0151e805f7446ed1d5ae990f5f0680f8b4b03e5a  apps-empty.ppm
895d13c55341090c408fa658ef89aca771dc4ae32a8d741d8cefe2194cb71e70  apps.ppm
51d593360c9ec9f091537181168bc45be67270e3a09969a7b1d94ee78c74bc39  device-diagnostics.ppm
6b3086625deb17dcad52cff31bd44c781f21535b21a12570bdbd7d4a830ca985  device-power.ppm
b319774e90186f6bc6771144a9d766efbb19144699e54bb81c1f66178e323527  device-resources.ppm
8fb8a471d993042126e54269fb44e9d5b52805e47310f72ebf408c60788612f4  device-unavailable.ppm
f79f05120dbf9de95c8489d66c3020b7f4d36640829eb79d899582929c38562c  device.ppm
b30227204599548f1de899e863e80589155694b5f3240b8c598c1396a3d21c76  document.ppm
40258e54baae3a7a41765f2e95dc81f051765f0e1f89b4f654fd7152b0ebc3e5  home.ppm
d0f6c4fb530e418af657af2cf5297d52cbe42b612eb1005537fc855c63be6dae  network-detail.ppm
2962d9d9626435eb9b4dbf41a8cd80cab8c213e73bb81c3c9142ee7e411d6d5d  network-offline.ppm
b99153076a10fd8da5eb18448a195447a4b4dd65f1422403cc3a365bc889f738  network.ppm
5f1f3520d8c70b18b52b98c845d0ee06390ac6f30de6d44c293dc5587bbdb213  notification.ppm
3c9f90a8bcc0c5d5ffaad46d31748dd831fface44e7615083e1e8357b63256a6  permission.ppm
a6e5f954c77d1512c6abdd25d2b28a836983423a3ceb0990d014282915eff406  power.ppm
ee19e775aa1a59808b955b5cd51a40642dc86b20a920fab042ab986e7a2952f9  settings-apps-privacy.ppm
40b70e894b0e25733524acdcaca4f97d0a6e00f62cf95db6ee473522b1a375ff  settings-auto-update.ppm
a48887a13b1023ac784ed74828a2e54fb71b1fd36ca2a8d831dc4cb546e56098  settings-camera.ppm
e215ec94d5623d91197c512594fb1b3543fe8bc483d9918c999ea9c3d12efa8f  settings-confirm.ppm
8bf9615c80bc7bbbbda0b20323da7076db653936abfef0754d0ab77d880dbf5f  settings-connectivity.ppm
4409d709a14a9021a4e681885996ccd59dc712967f31fa978cf008b139a3df6f  settings-display.ppm
201fb57e7789ecd54d5fc9f09e56d63e46956aa12a09f06ba673c7e14653a234  settings-power.ppm
6373e4850ceb18989705095fc2c6036788eb82978b7e83aa3dc9f97f4b1e1c97  settings-security.ppm
f10e4e270371f6eaab38e4125f37e40f1fb9a17c3b5ef5473425e7b057fd3be4  settings-sound.ppm
fb1ac2961e301b90c29719745d313c17910f4c938a7a03992abb4fc1e225e3ef  settings-system.ppm
cc4e5bbf3f6dee26b0b6514661481c9217abe354a1dfa3459eac8cc7c7b27340  settings.ppm
22ab6d4b687a348cd5fc6f045aa71f7638258a530ed0673be62661364cbcd0c3  store-background-progress.ppm
4c6117c8ed1a6ceb6555f2e058d7a42892f03f5fc5dc3c29d59890e803a13fab  store-description.ppm
714a26b2be741003e0699b8b20e718c454168655db52a7ba5c36e7656c711d94  store-detail.ppm
0d206108475fe5ed7b3e5bb59f500b3926f5c19c02359962e9aa145c3fdde3f1  store-failed.ppm
e9906b64a47d9e7759205db7223fda3a6c2f6479f74d207072ce1ffb3f00ee69  store-install-confirm.ppm
d0395c830145a281575d9ce37bf7007450dc31e7f4b922046e6dc897eda63b37  store-install-storage.ppm
592c72d76f15abd7db0cc6d172376b65eace4554215ec0f8eae56f8746bad881  store-permissions.ppm
1326fd73dc44ffa1424cee18058783d3f3cb9f0885c0a82217d2085722a49237  store-release-notes.ppm
77168cfa3d1c0fb9d4840aa4e03a03d852a7565f4d96c4add814ad15a7af64a8  store-screenshot.ppm
acc6d50ba0ad6ab34f606b42708090e27f440632376199f8fe45ad40a342a104  store-search-empty.ppm
cfd18ef5f9110ee2c908d09ca62720b1f37e3d020478f8842d57b2d3b61b51cb  store-search-max.ppm
67083cf393e6e9e477cf5026b8c19c5fa14dca9d596599fce2f75c990f340e98  store-search-none.ppm
66147ca2c4d5a1b657c7edadbd67ef7adea421a0338b3522687c0c21e2b52e97  store-search-recent.ppm
3b6e5012e93115b55fbe224ed49d6057404ada973bbbb1ca604d898c2cc3c7af  store-search.ppm
54f4f4baed4d74492ce1940a0864c66522b0215eebbb70719a826c4cc43c95cf  store-updates.ppm
1dd93ca475967d20190e96bb9eaacef42e9f674de162c39ab15c8004d78e4bf3  store.ppm
c8f03c8a917c9dfb0cecd57f32df1e6e11d913a75480e5ca11c2a35b04b0d62e  system-brightness.ppm
34e659c664795fb8b2cef00fd6fd168c3fdf6e845421342b29cc41dc97b88558  system-help.ppm
a6035c53a535cab44d6065cbd27f70a57d2655ee1e13baf24c87ef9176e40239  theme-high-contrast.ppm
2bfce46d55e7ef494607fe0d42b7e0d698e2c7470427b62e3fa300462bf0f4cc  theme-light.ppm
b382c359864c04060e4676c13c50e9578d241f132009a2840a3a9ed8324cfae2  tasks.ppm'

if [ "$actual" != "$expected" ]; then
    echo "System Shell screenshot regression:" >&2
    echo "$actual" >&2
    exit 1
fi
