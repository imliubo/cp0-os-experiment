#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

"$repo_root/scripts/build-malicious-apps.sh"
node "$repo_root/tests/inspect-malicious-wasm.mjs" \
    "$repo_root/target/malicious/memory-hog.wasm" \
    "$repo_root/target/malicious/ambient-authority.wasm"

if cargo run --quiet -p cp0ctl -- manifest validate \
    "$repo_root/tests/malicious/path-escape-app.json" >/dev/null 2>&1; then
    echo "error: path-escaping malicious manifest was accepted" >&2
    exit 1
fi
