#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/test-tmp/runtime-display-test"
input_output="$repo_root/target/test-tmp/runtime-input-test"
broker_output="$repo_root/target/test-tmp/runtime-broker-test"
mkdir -p "$(dirname "$output")"

${HOST_CC:-cc} -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/app-runtime/src" \
    "$repo_root/tests/runtime-display-test.c" \
    "$repo_root/app-runtime/src/pixels.c" \
    -o "$output"
"$output"

${HOST_CC:-cc} -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/app-runtime/src" \
    "$repo_root/tests/runtime-input-test.c" \
    "$repo_root/app-runtime/src/input_queue.c" \
    -o "$input_output"
"$input_output"

${HOST_CC:-cc} -std=c11 -Wall -Wextra -Werror \
    -I"$repo_root/app-runtime/src" \
    "$repo_root/tests/runtime-broker-test.c" \
    "$repo_root/app-runtime/src/broker_client.c" \
    -o "$broker_output"
"$broker_output"
