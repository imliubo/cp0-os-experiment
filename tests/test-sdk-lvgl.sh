#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/test-tmp/sdk-lvgl"
mkdir -p "$output"

EM_CACHE="$output/em-cache" emcc -std=c11 -ffreestanding \
    -Wall -Wextra -Werror \
    -I"$repo_root/sdk/c/include" \
    -I"$repo_root/sdk/lvgl" \
    -I"$repo_root/tests/fixtures/lvgl" \
    -c "$repo_root/sdk/lvgl/cardputerzero_lvgl.c" \
    -o "$output/cardputerzero_lvgl.o"

file "$output/cardputerzero_lvgl.o" | grep -q 'WebAssembly'
