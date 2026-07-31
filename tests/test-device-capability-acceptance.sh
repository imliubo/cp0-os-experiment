#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
device_script="$repo_root/scripts/device-capability-acceptance.sh"
build_script="$repo_root/scripts/build-device-capability-apps.sh"
output="$repo_root/target/capability-simulator"

bash -n "$device_script" "$build_script"
grep -q 'cardputerzero-stability-acceptance.service' "$device_script"
grep -q 'dev.cardputerzero.acceptance' "$device_script"
grep -q 'dev.cardputerzero.isolation' "$device_script"
grep -q 'permission resolve' "$device_script"
grep -q 'audio-play=denied;audio-capture=denied;gpio=denied' "$device_script"
grep -q 'storage-isolation=ok' "$device_script"
grep -q '660:root:cp0-gpio' "$device_script"
grep -q '600:cp0-storage:cp0-storage' "$device_script"
if grep -q 'CP0_BROKER_SOCKET' "$device_script"; then
    echo "error: device acceptance bypasses application identity through the broker socket" >&2
    exit 1
fi

mkdir -p "$output"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/device-capability-probe" \
    --duration 5000 --permissions allow --keys '' \
    --output "$output/capability.ppm" --profile "$output/capability.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/storage-isolation-probe" \
    --duration 1000 --permissions allow --keys '' \
    --output "$output/isolation.ppm" --profile "$output/isolation.json"

for image in "$output/capability.ppm" "$output/isolation.ppm"; do
    head -n 1 "$image" | grep -qx P6
done
jq -e '
    .frames_presented >= 1 and
    .storage_bytes == (14 + (.notifications[0].body | length)) and
    .storage_keys == 2 and
    .capability_calls["audio.playback"] == 1 and
    .capability_calls["audio.capture"] == 1 and
    .capability_calls["hardware.gpio"] == 5 and
    .capability_calls["notifications.post"] == 1 and
    .notifications == [{
      "title": "CP0 Capability Probe",
      "body": "audio-play=ok;audio-capture=ok-silent;gpio=ok;storage=quota-ok-new"
    }]
' "$output/capability.json" >/dev/null
jq -e '
    .frames_presented >= 1 and
    .storage_bytes == (.notifications[0].body | length) and
    .storage_keys == 1 and
    .notifications == [{
      "title": "CP0 Isolation Probe",
      "body": "storage-isolation=ok"
    }]
' "$output/isolation.json" >/dev/null
