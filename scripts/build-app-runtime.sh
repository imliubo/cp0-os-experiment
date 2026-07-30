#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source "$repo_root/app-runtime/wamr.env"
source "$repo_root/app-runtime/wayland.env"

source_dir="$repo_root/target/vendor/wasm-micro-runtime"
wayland_dir="$repo_root/target/vendor/wayland"
wayland_protocols_dir="$repo_root/target/vendor/wayland-protocols"
libffi_dir="$repo_root/target/vendor/libffi"
libffi_build="$repo_root/target/libffi-aarch64"
libffi_install="$libffi_build/install"
scanner_build="$repo_root/target/wayland-scanner-host"
protocol_build="$repo_root/target/app-runtime-protocol"
build_dir="$repo_root/target/app-runtime-aarch64"

ensure_checkout() {
    repository=$1
    commit=$2
    checkout=$3
    name=$4

    if [ ! -d "$checkout/.git" ]; then
        mkdir -p "$(dirname "$checkout")"
        git clone --filter=blob:none --no-checkout "$repository" "$checkout"
        git -C "$checkout" checkout "$commit"
    fi
    actual_commit=$(git -C "$checkout" rev-parse HEAD)
    if [ "$actual_commit" != "$commit" ]; then
        echo "error: $name checkout is $actual_commit, expected $commit" >&2
        exit 1
    fi
}

ensure_checkout "$WAMR_REPOSITORY" "$WAMR_COMMIT" "$source_dir" WAMR
ensure_checkout "$WAYLAND_REPOSITORY" "$WAYLAND_COMMIT" \
    "$wayland_dir" Wayland
ensure_checkout "$WAYLAND_PROTOCOLS_REPOSITORY" "$WAYLAND_PROTOCOLS_COMMIT" \
    "$wayland_protocols_dir" wayland-protocols
ensure_checkout "$LIBFFI_REPOSITORY" "$LIBFFI_COMMIT" "$libffi_dir" libffi

if [ ! -x "$libffi_dir/configure" ]; then
    (cd "$libffi_dir" && ./autogen.sh)
fi
mkdir -p "$libffi_build"
if [ ! -f "$libffi_build/Makefile" ]; then
    build_triplet=$($libffi_dir/config.guess)
    (cd "$libffi_build" && "$libffi_dir/configure" \
        --build="$build_triplet" \
        --host=aarch64-linux-gnu \
        --disable-shared \
        --enable-static \
        --disable-docs \
        --disable-multi-os-directory \
        --prefix="$libffi_install" \
        CC=aarch64-linux-gnu-gcc \
        AR=aarch64-linux-gnu-ar \
        RANLIB=aarch64-linux-gnu-ranlib)
fi
make -C "$libffi_build" --jobs "$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)" install
test -f "$libffi_install/lib/libffi.a"

mkdir -p "$scanner_build" "$protocol_build"
${HOST_CC:-cc} -std=c11 -O2 -DHAVE_LIBXML=0 \
    -I"$repo_root/app-runtime/protocol" \
    -I"$wayland_dir" \
    -I"$wayland_dir/src" \
    "$wayland_dir/src/scanner.c" \
    "$wayland_dir/src/wayland-util.c" \
    -lexpat \
    -o "$scanner_build/wayland-scanner"
scanner="$scanner_build/wayland-scanner"

"$scanner" client-header -c "$wayland_dir/protocol/wayland.xml" \
    "$protocol_build/wayland-client-protocol-core.h"
"$scanner" client-header "$wayland_dir/protocol/wayland.xml" \
    "$protocol_build/wayland-client-protocol.h"
"$scanner" private-code "$wayland_dir/protocol/wayland.xml" \
    "$protocol_build/wayland-protocol.c"
"$scanner" client-header \
    "$wayland_protocols_dir/stable/xdg-shell/xdg-shell.xml" \
    "$protocol_build/xdg-shell-client-protocol.h"
"$scanner" private-code \
    "$wayland_protocols_dir/stable/xdg-shell/xdg-shell.xml" \
    "$protocol_build/xdg-shell-protocol.c"
install -m 0644 "$repo_root/app-runtime/protocol/wayland-config.h" \
    "$wayland_dir/config.h"

cmake -S "$repo_root/app-runtime" -B "$build_dir" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_TOOLCHAIN_FILE="$repo_root/app-runtime/cmake/aarch64-linux-gnu.cmake" \
    -DWAMR_ROOT_DIR="$source_dir" \
    -DWAYLAND_ROOT_DIR="$wayland_dir" \
    -DWAYLAND_PROTOCOL_DIR="$protocol_build" \
    -DLIBFFI_ROOT_DIR="$libffi_install"
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
