#!/usr/bin/env bash
set -euo pipefail

if (($# < 1 || $# > 5)); then
    echo "usage: verify-app.sh APP_DIR [KEYS] [allow|deny] [DURATION_MS] [MEDIA_ACTIONS]" >&2
    exit 2
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
root=${CP0_DEVKIT_ROOT:-$script_dir/../../..}
root=$(cd "$root" && pwd -P)
app=$1
keys=${2:-}
permissions=${3:-deny}
duration=${4:-750}
media_actions=${5:-}
case "$permissions" in
    allow | deny) ;;
    *)
        echo "error: permissions must be allow or deny" >&2
        exit 2
        ;;
esac
if [[ ! $duration =~ ^[0-9]+$ ]] || ((duration < 100 || duration > 30000)); then
    echo "error: duration must be between 100 and 30000 milliseconds" >&2
    exit 2
fi
if [[ ! -d $app ]]; then
    echo "error: application directory does not exist: $app" >&2
    exit 1
fi
app=$(cd "$app" && pwd -P)

"$script_dir/doctor.sh" "$root" rust >/dev/null
pinned_rust=$(awk -F '"' '$1 ~ /^rust_version = / { print $2 }' \
    "$root/devkit/toolchain.toml")
export RUSTUP_TOOLCHAIN=$pinned_rust
if [[ -x $root/bin/cp0ctl ]]; then
    cp0ctl=("$root/bin/cp0ctl")
elif [[ -f $root/Cargo.toml ]]; then
    cp0ctl=(cargo run --quiet --manifest-path "$root/Cargo.toml" -p cp0ctl --)
else
    echo "error: cp0ctl is missing from the DevKit" >&2
    exit 1
fi

output="$app/target/cardputerzero/skill-verification"
mkdir -p "$output"
frame="$output/frame.ppm"
profile="$output/profile.json"

"${cp0ctl[@]}" manifest validate "$app/app.json"
"${cp0ctl[@]}" build "$app"
run_arguments=(
    "$app"
    --duration "$duration"
    --permissions "$permissions"
    --keys "$keys"
    --output "$frame"
    --profile "$profile"
)
if [[ -n $media_actions ]]; then
    run_arguments+=(--media-actions "$media_actions")
fi
"${cp0ctl[@]}" run "${run_arguments[@]}"

node -e '
const fs = require("fs");
const [manifestPath, framePath, profilePath, mediaActions] = process.argv.slice(1);
const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
const profile = JSON.parse(fs.readFileSync(profilePath, "utf8"));
const frame = fs.readFileSync(framePath);
const header = frame.subarray(0, Math.min(frame.length, 64)).toString("ascii");
const height = manifest.display === "immersive" ? 170 : 150;
if (!header.startsWith(`P6\n320 ${height}\n255\n`)) throw new Error("invalid simulator frame dimensions");
if (profile.app_id !== manifest.id || profile.frames_presented < 1) throw new Error("invalid simulator profile");
const expectedMedia = mediaActions ? mediaActions.split(",") : [];
if (JSON.stringify(profile.scripted_media_actions || []) !== JSON.stringify(expectedMedia)) {
  throw new Error("simulator media action profile mismatch");
}
if (expectedMedia.length > 0 && profile.media_session_updates < 1) {
  throw new Error("application did not register a media session");
}
' "$app/app.json" "$frame" "$profile" "$media_actions"

printf 'PASS app=%s frame=%s profile=%s\n' "$app" "$frame" "$profile"
