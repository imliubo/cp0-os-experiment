#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repo_root/app-runtime/wamr.env"

source_dir="$repo_root/target/vendor/wasm-micro-runtime"
build_dir="$repo_root/target/app-runtime-aarch64"

if [ ! -d "$source_dir/.git" ]; then
    mkdir -p "$(dirname "$source_dir")"
    git clone --filter=blob:none --no-checkout "$WAMR_REPOSITORY" "$source_dir"
    git -C "$source_dir" checkout "$WAMR_COMMIT"
fi

actual_commit=$(git -C "$source_dir" rev-parse HEAD)
if [ "$actual_commit" != "$WAMR_COMMIT" ]; then
    echo "error: WAMR checkout is $actual_commit, expected $WAMR_COMMIT" >&2
    exit 1
fi

cmake -S "$repo_root/app-runtime" -B "$build_dir" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_TOOLCHAIN_FILE="$repo_root/app-runtime/cmake/aarch64-linux-gnu.cmake" \
    -DWAMR_ROOT_DIR="$source_dir"
cmake --build "$build_dir" --parallel

runtime="$build_dir/cardputerzero-app-runtime"
seccomp_probe="$build_dir/cp0-seccomp-probe"
test -x "$runtime"
test -x "$seccomp_probe"
file "$runtime" | grep -q 'ELF 64-bit LSB executable, ARM aarch64'
file "$seccomp_probe" | grep -q 'ELF 64-bit LSB executable, ARM aarch64'
if aarch64-linux-gnu-readelf -d "$runtime" | grep -q '(NEEDED)'; then
    echo "error: App Runtime must not have dynamic library dependencies" >&2
    exit 1
fi
if aarch64-linux-gnu-readelf -d "$seccomp_probe" | grep -q '(NEEDED)'; then
    echo "error: seccomp probe must not have dynamic library dependencies" >&2
    exit 1
fi
sha256sum "$runtime"
sha256sum "$seccomp_probe"
