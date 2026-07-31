#!/usr/bin/env bash
set -euo pipefail

duration_seconds=${1:-5}
if [[ ! $duration_seconds =~ ^[1-9][0-9]*$ ]] || ((duration_seconds > 3600)); then
    echo "usage: $0 [SECONDS_PER_TARGET:1..3600]" >&2
    exit 2
fi

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
local_tools="$repo_root/target/fuzz-tools/bin"
if [[ -x $local_tools/cargo-fuzz ]]; then
    export PATH="$local_tools:$PATH"
fi
if ! command -v cargo-fuzz >/dev/null 2>&1; then
    echo "error: cargo-fuzz is required; install it globally or under target/fuzz-tools" >&2
    exit 1
fi
if ! rustup run nightly rustc --version >/dev/null 2>&1; then
    echo "error: the Rust nightly toolchain is required for libFuzzer" >&2
    exit 1
fi

targets=(manifest package store_protocol appd_control recovery_backup)
for target in "${targets[@]}"; do
    echo "Fuzzing $target for ${duration_seconds}s"
    cargo +nightly fuzz run --fuzz-dir "$repo_root/fuzz" "$target" -- \
        -max_total_time="$duration_seconds" \
        -timeout=5 \
        -max_len=65536 \
        -rss_limit_mb=1536
done
