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
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/shell_settings.c" \
    "$repo_root/tests/system-shell-settings.c" \
    -o "$work_dir/system-shell-settings-test"
"$work_dir/system-shell-settings-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_APPD_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/appd_client.c" \
    "$repo_root/tests/system-shell-appd-client.c" \
    -o "$work_dir/system-shell-appd-client-test"
"$work_dir/system-shell-appd-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_DEVELOPER_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/developer_client.c" \
    "$repo_root/tests/system-shell-developer-client.c" \
    -o "$work_dir/system-shell-developer-client-test"
"$work_dir/system-shell-developer-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_POWER_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/power_client.c" \
    "$repo_root/tests/system-shell-power-client.c" \
    -o "$work_dir/system-shell-power-client-test"
"$work_dir/system-shell-power-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_DISPLAY_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/display_client.c" \
    "$repo_root/tests/system-shell-display-client.c" \
    -o "$work_dir/system-shell-display-client-test"
"$work_dir/system-shell-display-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_AUDIO_SETTINGS_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/audio_settings_client.c" \
    "$repo_root/tests/system-shell-audio-settings-client.c" \
    -o "$work_dir/system-shell-audio-settings-client-test"
"$work_dir/system-shell-audio-settings-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_CONNECTIVITY_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/connectivity_client.c" \
    "$repo_root/tests/system-shell-connectivity-client.c" \
    -o "$work_dir/system-shell-connectivity-client-test"
"$work_dir/system-shell-connectivity-client-test"

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -DCP0_PROVISION_CLIENT_TEST \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/json.c" \
    "$repo_root/system-shell/src/provision_client.c" \
    "$repo_root/tests/system-shell-provision-client.c" \
    -o "$work_dir/system-shell-provision-client-test"
"$work_dir/system-shell-provision-client-test"

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

"${CC:-cc}" -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/system-shell/include" \
    "$repo_root/system-shell/src/screenshot_store.c" \
    "$repo_root/tests/system-shell-screenshot-store.c" \
    $(pkg-config --cflags --libs libpng) \
    -o "$work_dir/system-shell-screenshot-store-test"
"$work_dir/system-shell-screenshot-store-test" "$work_dir/screenshot-store"

snapshot_files='app-actions.ppm app-overview.ppm app-permissions.ppm app-storage.ppm app-uninstall.ppm apps-empty.ppm apps.ppm device-diagnostics.ppm device-power.ppm device-resources.ppm device-unavailable.ppm device.ppm document.ppm home.ppm network-detail.ppm network-offline.ppm network.ppm notification.ppm permission.ppm power.ppm settings-apps-privacy.ppm settings-auto-update.ppm settings-camera.ppm settings-confirm.ppm settings-connectivity.ppm settings-developer-hosts.ppm settings-developer-revoke.ppm settings-display.ppm settings-metrics-confirm.ppm settings-metrics.ppm settings-power.ppm settings-security.ppm settings-sound.ppm settings-system.ppm settings.ppm setup-busy.ppm setup-complete.ppm setup-hostname.ppm setup-network.ppm setup-password.ppm setup-repair.ppm setup-review.ppm setup-welcome-waiting.ppm setup-welcome.ppm setup-wifi.ppm store-background-progress.ppm store-description.ppm store-detail.ppm store-failed.ppm store-install-confirm.ppm store-install-storage.ppm store-permissions.ppm store-release-notes.ppm store-screenshot.ppm store-search-empty.ppm store-search-max.ppm store-search-none.ppm store-search-recent.ppm store-search.ppm store-today-collection.ppm store-today.ppm store-updates.ppm store.ppm system-brightness.ppm system-help.ppm system-media-busy.ppm system-media-failed.ppm system-media-sent.ppm system-media-unavailable.ppm system-screenshot-saved.ppm system-screenshot-unavailable.ppm theme-high-contrast.ppm theme-light.ppm tasks.ppm'
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(cd "$snapshot_dir" && sha256sum $snapshot_files)
else
    actual=$(cd "$snapshot_dir" && shasum -a 256 $snapshot_files)
fi

expected='b252a095d38eac93567ff1d5c8d2fa8edee762151cc72b442616ae59a60eeb3f  app-actions.ppm
7449565bc568403d51d8dadc9743193a8c97f85d32feb772d263863a83bcedcc  app-overview.ppm
e5854e36cef4114241b0ba61cd4a7c4192e023726388c63357f31d27f982ff5f  app-permissions.ppm
bc829a4605fdee296b28bf5949812daa3a39b455929ba0201987d856d5d1a3a2  app-storage.ppm
d5cf7400f7a9475a611ba8bc19b3316ba18e0ee26597785308726f33a1b9d837  app-uninstall.ppm
e690bfa55247afebf1858fbf0151e805f7446ed1d5ae990f5f0680f8b4b03e5a  apps-empty.ppm
eedd4433e670e42e2bcb2cfb8226e645b2a32a84a8ea246b6141e2c1c90837fe  apps.ppm
51d593360c9ec9f091537181168bc45be67270e3a09969a7b1d94ee78c74bc39  device-diagnostics.ppm
6b3086625deb17dcad52cff31bd44c781f21535b21a12570bdbd7d4a830ca985  device-power.ppm
b319774e90186f6bc6771144a9d766efbb19144699e54bb81c1f66178e323527  device-resources.ppm
8fb8a471d993042126e54269fb44e9d5b52805e47310f72ebf408c60788612f4  device-unavailable.ppm
f79f05120dbf9de95c8489d66c3020b7f4d36640829eb79d899582929c38562c  device.ppm
be31b0f7cee2ecde9c9d6b5521af41686019f0873fb7f88398174976db85f434  document.ppm
40258e54baae3a7a41765f2e95dc81f051765f0e1f89b4f654fd7152b0ebc3e5  home.ppm
d0f6c4fb530e418af657af2cf5297d52cbe42b612eb1005537fc855c63be6dae  network-detail.ppm
efa8b35b22372eaf879a4e47ad5b5b7c30247ce008738a979fddabb78b389c37  network-offline.ppm
653692ea351514d9813b17ca261790d546c45404bf95ce5a6056e993acc38f6c  network.ppm
9339f99f3b7134f1df3089248ecfacc60a7f461a01971eda10c780360ac2f1ec  notification.ppm
5ad4e7360ef38b27d0a416e62840e42b8aa88aaccdfe7aa54da944659b261728  permission.ppm
8b0cfe3da3a68f5c4ebd8138eb56c024a06c4261581430a1ab3bf5e0a62d082b  power.ppm
3decae67fc0f937c1c97d6585395a98525233494ae7d888cc50cdea2a6da3cc5  settings-apps-privacy.ppm
72d7f83bd1073ae5591440c5f338b2c4e01a0ab21fddaf1c4c09550fcf98f2f6  settings-auto-update.ppm
699ca1a9ad4d58a27d64c6611f59aa31d9a21d1ea9d311fb68a0f1980ef46c24  settings-camera.ppm
e215ec94d5623d91197c512594fb1b3543fe8bc483d9918c999ea9c3d12efa8f  settings-confirm.ppm
e158ca788806ea1887c95e25e293c8b739526fcfb9c10e6e795e63e18afcc7b8  settings-connectivity.ppm
ed804e0d53f864bf97230a887a7bf79ef569a3c68d45d4a9bdf4ad8051bdc71a  settings-developer-hosts.ppm
10c023bc11a77979006543897e8ea3a87eb43e4179e81ecdd26530fc6d698222  settings-developer-revoke.ppm
4409d709a14a9021a4e681885996ccd59dc712967f31fa978cf008b139a3df6f  settings-display.ppm
ddf7f0cc652be525f53f38bfb8126aadd1d3935f90deacd753083ca6b55a2035  settings-metrics-confirm.ppm
563737565034c5e0a27e23f347a0e15605add45a95049f88a8436ba8a80359be  settings-metrics.ppm
524cee8a9b59415913a1be9b59d41b54477a2424fc31c8ab7c060d86660dc042  settings-power.ppm
d687ea2ca4ddb8c624e7c93c14610ac6f83b7160ec9e631f309cdca1b4d54d6c  settings-security.ppm
f10e4e270371f6eaab38e4125f37e40f1fb9a17c3b5ef5473425e7b057fd3be4  settings-sound.ppm
990b0797662f4cc621bf9510fabc3414babb7c5b058ec963d6957d4041a5d4d1  settings-system.ppm
e88bf66b5f3233c8a62b97f5e84323a3a37a3b1d366b4fdeb5e31fd0851e2d23  settings.ppm
7c09a045b4556b158274414b1371c4129e65f3628e6787adc815a4463f2b5073  setup-busy.ppm
1e747cc74f84757736ccf08593d9a3754b2cc32805588eb650743cbbbb005f33  setup-complete.ppm
81d50d7a4ac849fd8b289ebe0efd857a621bbb5ca2c9b2cd2fd5a5793a51a008  setup-hostname.ppm
654dbe56046fdf713b8bde8c765ddb7de3f763c465cd9e629024023ade0788c6  setup-network.ppm
fc678e495c5757296bd00bcd267071970dc8ca7191675f05210cf4d2a5f9b325  setup-password.ppm
ea33057e57e476160109bbb950eae3896709447168e60850c08626968c8a14e0  setup-repair.ppm
85298ad8509106ba7982927b6a76d1c6fee768e1c19969dc1a0ac2345037d7e8  setup-review.ppm
13d560a11c33c00c40e2e32fff818fe15c5b2f63fc865a8f192846910c743c5e  setup-welcome-waiting.ppm
13d560a11c33c00c40e2e32fff818fe15c5b2f63fc865a8f192846910c743c5e  setup-welcome.ppm
f3619413bdceecd9c217eb379cec9ff69aa0fb1066ceda0dd065f1138c6ab9b5  setup-wifi.ppm
22ab6d4b687a348cd5fc6f045aa71f7638258a530ed0673be62661364cbcd0c3  store-background-progress.ppm
ecbd3ca42a3d5fc772edbf1826553a595ad7bb65993f958fd8d58bf409ef0d9f  store-description.ppm
a141091b9002b8c067264ff89667a1813ea168b57f0363f7daf6f42423cfb2b8  store-detail.ppm
7911d2f01a0b2a26d9c711ca1c6061616d5a7c610d1f2dd7c7edfffb25c5aeb9  store-failed.ppm
82eecc99d2ce4ec25a147a2b995159376f07fa8028812f663bfe659484a8521d  store-install-confirm.ppm
d0395c830145a281575d9ce37bf7007450dc31e7f4b922046e6dc897eda63b37  store-install-storage.ppm
592c72d76f15abd7db0cc6d172376b65eace4554215ec0f8eae56f8746bad881  store-permissions.ppm
222e8e83f3838a78d012a9a706ba966bea5bafc077d7a7b8379191437cf46829  store-release-notes.ppm
77168cfa3d1c0fb9d4840aa4e03a03d852a7565f4d96c4add814ad15a7af64a8  store-screenshot.ppm
9818bf7880f25774384ccfff590781480fdbbc665a182cbb1328adc44f11adef  store-search-empty.ppm
a321a9c3ccdbc1c1e3d271d4760136c59b68c9803348845eaceac3b1e1665d25  store-search-max.ppm
8036f811a10fa8348f829b221f9c6e1541406c32e3e749b69f8df854ff6f0f1a  store-search-none.ppm
c022363b6a89d06b912ebf02a32df2ded1c6bacf2e64c55def98d63de9adadda  store-search-recent.ppm
d6fc889166a3778068ee0e668a625948e99d58078d1d84e71e5e1db7f6b1ea25  store-search.ppm
02d302faff4949340c1dc688a13e56f0c360e5e7f2a3a1fd2cc0213b4341c3d5  store-today-collection.ppm
7766deedd3cb1760d944554c9e53a1b3bd54970c1017e6833055c16430d1be3d  store-today.ppm
28abef5de1243bbc1d89db702e4641590d483b4e837f7a6b4a1f8d879aaac393  store-updates.ppm
086ad57262d4bd4a9e873ea915b695a3e789df3cee559fa5417f2bc040082012  store.ppm
a09e1d8b9dee19495466ad51abc51183cf549b0082810e1351fe040a5c21de3d  system-brightness.ppm
9836fec7dea7d6afae141cb3da3b394f8bf05b01d2ac624f43f8f02b8c29154d  system-help.ppm
30a3488c777f4ddfc509d645cc777a40e14b59faecdbccf781b79ce90a7b2f7f  system-media-busy.ppm
db2d525b769c4771bd8b7116ddddfc4672bd2b54cf8aaff06d390dd4796f9c6c  system-media-failed.ppm
e0e1d7447b8535d1b81e8e83cb5d6e2b33ef45ea1ccf1ca2eab0e2b94b16524a  system-media-sent.ppm
0798537254fc81829e9be72bfa8cba367f8da20c9f029879411fb1f7e1c26e6f  system-media-unavailable.ppm
abbfe8dea28bc05258ea8031051e2fad43fab8b99758942f2fe2ea0cf8ee6af0  system-screenshot-saved.ppm
f580566abdc0d1de33888bab69d77071ac4c767d56dbe4561e91944099e2aeac  system-screenshot-unavailable.ppm
1a0c201546f675d79e656e99dfe5cb254b251f4f5c92e3abf5036d1863e1e64e  theme-high-contrast.ppm
79e5c787c9b0cc012213f4ddc9a2e7615f20fd9f36d4698e8d6f2cf07aeb8998  theme-light.ppm
879c45ff089f2ef29fbbeb019199dfd4797d06c9bd4e01590b47d1c381f95d80  tasks.ppm'

if [ "$actual" != "$expected" ]; then
    echo "System Shell screenshot regression:" >&2
    echo "$actual" >&2
    exit 1
fi
