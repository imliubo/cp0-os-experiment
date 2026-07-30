#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
output="$repo_root/target/test-tmp/sdk-c"
mkdir -p "$output"

EM_CACHE="$output/em-cache" emcc -std=c11 -ffreestanding \
    -Wall -Wextra -Werror -I"$repo_root/sdk/c/include" \
    -c "$repo_root/tests/sdk-c-smoke.c" -o "$output/sdk-c-smoke.o"
EM_CACHE="$output/em-cache" em++ -std=c++17 -ffreestanding \
    -fno-exceptions -fno-rtti -Wall -Wextra -Werror \
    -I"$repo_root/sdk/c/include" \
    -c "$repo_root/tests/sdk-cxx-smoke.cc" -o "$output/sdk-cxx-smoke.o"

file "$output/sdk-c-smoke.o" | grep -q 'WebAssembly'
file "$output/sdk-cxx-smoke.o" | grep -q 'WebAssembly'
