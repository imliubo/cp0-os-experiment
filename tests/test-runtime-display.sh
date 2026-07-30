#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/test-tmp/runtime-display-test"
mkdir -p "$(dirname "$output")"

${HOST_CC:-cc} -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/app-runtime/src" \
    "$repo_root/tests/runtime-display-test.c" \
    "$repo_root/app-runtime/src/pixels.c" \
    -o "$output"
"$output"
