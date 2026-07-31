#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/simulator-test/frame.ppm"
profile="$repo_root/target/simulator-test/profile.json"

node --check "$repo_root/simulator/cp0-simulator.mjs"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/hello-card" \
    --duration 250 \
    --permissions allow \
    --keys c,g,s \
    --output "$output" \
    --profile "$profile"

head -n 1 "$output" | grep -qx 'P6'
jq -e '
    .app_id == "dev.cardputerzero.hello" and
    .frames_presented >= 1 and
    .key_events == 3 and
    .memory_pages > 0 and
    .capability_calls["camera.capture"] == 1 and
    .capability_calls["hardware.gpio"] == 2
' "$profile" >/dev/null
