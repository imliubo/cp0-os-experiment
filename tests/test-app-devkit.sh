#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
test_parent="$repo_root/target/test-tmp"
mkdir -p "$test_parent"
test_root=$(mktemp -d "$test_parent/cp0-app-devkit.XXXXXX")
case "$test_root" in
    "$test_parent"/cp0-app-devkit.*) ;;
    *)
        echo "error: unsafe App DevKit test directory" >&2
        exit 1
        ;;
esac
trap 'rm -rf -- "$test_root"' EXIT

"$repo_root/scripts/package-app-devkit.sh" "$test_root/release" >/dev/null
archives=("$test_root"/release/cardputerzero-app-devkit-*.tar.xz)
if ((${#archives[@]} != 1)) || [[ ! -f ${archives[0]} ]]; then
    echo "error: App DevKit packager did not produce exactly one archive" >&2
    exit 1
fi
archive=${archives[0]}
archive_name=$(basename "$archive")
test "$(awk '{ print $2 }' "$archive.sha256")" = "$archive_name"
(
    cd "$(dirname "$archive")"
    shasum -a 256 -c "$archive_name.sha256" >/dev/null
)
if tar -tJf "$archive" | grep -Eq '(^|/)(\.DS_Store|\._[^/]+)$'; then
    echo "error: App DevKit archive contains host metadata" >&2
    exit 1
fi
mkdir -p "$test_root/unpacked"
tar -C "$test_root/unpacked" -xJf "$archive"
roots=("$test_root"/unpacked/cardputerzero-app-devkit-*)
if ((${#roots[@]} != 1)) || [[ ! -d ${roots[0]} ]]; then
    echo "error: App DevKit archive has an invalid root" >&2
    exit 1
fi
devkit=${roots[0]}
(
    cd "$devkit"
    shasum -a 256 -c SHA256SUMS >/dev/null
)
jq -e '.version == "1.0.0" and (.built_with.rust | startswith("rustc 1.85.1 "))' \
    "$devkit/devkit.json" >/dev/null
test -s "$devkit/schemas/store-listing-v1.schema.json"
test -s "$devkit/docs/DEVELOPER-ACCESS.md"
jq -e '.bundled | index("developer-access-doc") != null' \
    "$devkit/devkit.json" >/dev/null

"$devkit/skills/cardputerzero-build-app/scripts/doctor.sh" "$devkit" rust >/dev/null
env RUSTUP_TOOLCHAIN=1.85.1 "$devkit/bin/cp0ctl" new \
    "$test_root/generated" dev.cardputerzero.generated \
    "Generated" >/dev/null
grep -Fq "$devkit/sdk/rust" "$test_root/generated/Cargo.toml"
env RUSTUP_TOOLCHAIN=1.85.1 "$devkit/bin/cp0ctl" \
    build "$test_root/generated" >/dev/null
env RUSTUP_TOOLCHAIN=1.85.1 \
    "$devkit/skills/cardputerzero-build-app/scripts/verify-app.sh" \
    "$devkit/examples/neon-snake" up,left,down,right,space,space deny 2400 >/dev/null
env RUSTUP_TOOLCHAIN=1.85.1 \
    "$devkit/skills/cardputerzero-build-app/scripts/verify-app.sh" \
    "$devkit/examples/media-controls" "" deny 600 \
    play-pause,previous,next >/dev/null

test -s "$devkit/examples/neon-snake/target/cardputerzero/skill-verification/frame.ppm"
test -s "$devkit/examples/neon-snake/target/cardputerzero/skill-verification/profile.json"
jq -e '
    .scripted_media_actions == ["play-pause", "previous", "next"] and
    .media_actions_taken == 3 and
    .media_session_updates == 4
' "$devkit/examples/media-controls/target/cardputerzero/skill-verification/profile.json" \
    >/dev/null
echo "PASS relocatable CardputerZero App DevKit"
