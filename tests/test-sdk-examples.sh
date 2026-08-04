#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/example-simulator"
mkdir -p "$output"

for manifest in "$repo_root"/examples/*/app.json; do
    example_dir=$(dirname "$manifest")
    test -s "$example_dir/README.md"
    grep -Fq 'assets/screenshot.png' "$example_dir/README.md"
    test -s "$example_dir/assets/screenshot.png"
done

cargo fmt --manifest-path "$repo_root/examples/neon-snake/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/neon-snake/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/calculator/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/calculator/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/camera/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/camera/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/gallery/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/gallery/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/media-controls/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/media-controls/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/notes/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/notes/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/stopwatch/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/stopwatch/Cargo.toml"
cargo fmt --manifest-path "$repo_root/examples/keyboard-diagnostics/Cargo.toml" -- --check
cargo test --quiet --manifest-path "$repo_root/examples/keyboard-diagnostics/Cargo.toml"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/calculator" \
    --duration 250 --permissions deny --keys 1,2,plus,3,equal \
    --output "$output/calculator.ppm" --profile "$output/calculator.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/camera" \
    --duration 700 --permissions allow --keys enter \
    --output "$output/camera.ppm" --profile "$output/camera.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/gallery" \
    --duration 250 --permissions allow \
    --output "$output/gallery.ppm" --profile "$output/gallery.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/notes" \
    --duration 900 --permissions deny --keys h,e,l,l,o \
    --output "$output/notes.ppm" --profile "$output/notes.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/stopwatch" \
    --duration 350 --permissions deny --keys enter,r,space \
    --output "$output/stopwatch.ppm" --profile "$output/stopwatch.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/media-controls" \
    --duration 600 --permissions deny \
    --media-actions play-pause,previous,next \
    --output "$output/media-controls.ppm" --profile "$output/media-controls.json"
cargo run --quiet --manifest-path "$repo_root/Cargo.toml" -p cp0ctl -- \
    run "$repo_root/examples/neon-snake" \
    --duration 2400 --permissions deny \
    --keys up,left,down,right,space,space \
    --output "$output/neon-snake.ppm" --profile "$output/neon-snake.json"

for image in \
    "$output/calculator.ppm" \
    "$output/camera.ppm" \
    "$output/gallery.ppm" \
    "$output/media-controls.ppm" \
    "$output/neon-snake.ppm" \
    "$output/notes.ppm" \
    "$output/stopwatch.ppm"; do
    head -n 1 "$image" | grep -qx 'P6'
done
jq -e '.key_events == 5 and .frames_presented >= 2' \
    "$output/calculator.json" >/dev/null
jq -e '
    .key_events == 1 and
    .frames_presented >= 2 and
    .capability_calls["camera.capture"] >= 1 and
    .capability_calls["photos.write"] == 16 and
    .photo_library_keys == 15 and
    .photo_library_bytes == 108816
' "$output/camera.json" >/dev/null
jq -e '
    .frames_presented >= 1 and
    .capability_calls["photos.read"] >= 1 and
    .photo_library_keys == 0
' "$output/gallery.json" >/dev/null
jq -e '
    .scripted_media_actions == ["play-pause", "previous", "next"] and
    .media_session_updates == 4 and
    .media_actions_taken == 3 and
    .frames_presented >= 3 and
    .capability_calls == {}
' "$output/media-controls.json" >/dev/null
jq -e '
    .key_events == 6 and
    .frames_presented >= 16 and
    .storage_bytes == 4 and
    .storage_keys == 1 and
    .capability_calls == {}
' "$output/neon-snake.json" >/dev/null
jq -e '
    .key_events == 5 and
    .frames_presented >= 2 and
    .storage_bytes == 5 and
    .storage_keys == 1 and
    .capability_calls == {}
' "$output/notes.json" >/dev/null
jq -e '
    .key_events == 3 and
    .frames_presented >= 1 and
    .capability_calls == {}
' "$output/stopwatch.json" >/dev/null

printf '%s\n' \
    'CP0K,1,1' \
    'S,1,HOLD SHIFT + A,30,1,65' \
    'C,1,30,0,97,0' \
    'K,1,0' \
    'D,1,0,1,0' >"$output/keyboard-diagnostics.log"
"$repo_root/scripts/analyze-keyboard-diagnostics.sh" \
    "$output/keyboard-diagnostics.log" >"$output/keyboard-diagnostics-analysis.txt"
grep -q '1 modifier-state mismatch' \
    "$output/keyboard-diagnostics-analysis.txt"
