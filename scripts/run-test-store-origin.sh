#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source_cloudflared="$repo_root/target/tools/cloudflared-2026.7.3/source/cloudflared"
source_cloudflared_sha256=0a59c7b61dedf9096d3df3ee52c7cef81ab31614e8fc8457e864506eae7aa672
sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        echo "error: shasum or sha256sum is required" >&2
        return 1
    fi
}

if [[ -e $source_cloudflared ]]; then
    if [[ ! -x $source_cloudflared ]] ||
        [[ $(sha256_file "$source_cloudflared") != "$source_cloudflared_sha256" ]]; then
        echo "error: repository-local cloudflared failed executable or SHA-256 verification" >&2
        exit 1
    fi
    cloudflared_bin=$source_cloudflared
elif [[ -n ${CP0_CLOUDFLARED:-} ]]; then
    if [[ $CP0_CLOUDFLARED != /* || ! -x $CP0_CLOUDFLARED ]]; then
        echo "error: CP0_CLOUDFLARED must be an absolute executable path" >&2
        exit 1
    fi
    cloudflared_bin=$CP0_CLOUDFLARED
elif cloudflared_bin=$(command -v cloudflared 2>/dev/null); then
    :
else
    echo "error: no trusted cloudflared executable was found" >&2
    exit 1
fi

if [[ ${1:-} == --print-cloudflared ]]; then
    if (($# != 1)); then
        echo "usage: run-test-store-origin --print-cloudflared" >&2
        exit 2
    fi
    printf '%s\n' "$cloudflared_bin"
    exit 0
fi

if (($# > 3)); then
    echo "usage: run-test-store-origin [PORT] [LIFETIME_SECONDS] [BYTES_PER_SECOND]" >&2
    exit 2
fi
port=${1:-18080}
lifetime=${2:-1800}
rate=${3:-524288}
if [[ ! $port =~ ^[1-9][0-9]*$ || ! $lifetime =~ ^[1-9][0-9]*$ ||
    ! $rate =~ ^[1-9][0-9]*$ ]] || ((port > 65535)) ||
    ((lifetime < 120 || lifetime > 2678400)) ||
    ((rate < 4096 || rate > 4194304)); then
    echo "error: port, lifetime or throttle rate is outside acceptance bounds" >&2
    exit 2
fi

for dependency in jq node; do
    if ! command -v "$dependency" >/dev/null 2>&1; then
        echo "error: $dependency is required" >&2
        exit 1
    fi
done

output="$repo_root/target/test-store"
control="$output/origin-control.json"
runtime_root="$repo_root/target/test-store-origin"
run_id=$(date -u +%Y%m%dT%H%M%SZ)-$$
run_dir="$runtime_root/$run_id"
ready="$run_dir/origin-ready.json"
origin_log="$run_dir/origin.log"
tunnel_log="$run_dir/cloudflared.log"
runtime="$run_dir/runtime.json"
mkdir -p "$output" "$run_dir"

node "$repo_root/scripts/test-store-origin.mjs" \
    set "$control" v1-slow "$rate" >/dev/null

origin_pid=
tunnel_pid=
cleanup() {
    if [[ -n $tunnel_pid ]]; then
        kill "$tunnel_pid" 2>/dev/null || :
        wait "$tunnel_pid" 2>/dev/null || :
    fi
    if [[ -n $origin_pid ]]; then
        kill "$origin_pid" 2>/dev/null || :
        wait "$origin_pid" 2>/dev/null || :
    fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

CP0_TEST_STORE_READY_FILE="$ready" \
    node "$repo_root/scripts/test-store-origin.mjs" \
    serve "$output" "$control" "$port" >"$origin_log" 2>&1 &
origin_pid=$!
for attempt in $(seq 1 100); do
    [[ -s $ready ]] && break
    if ! kill -0 "$origin_pid" 2>/dev/null; then
        cat "$origin_log" >&2
        exit 1
    fi
    sleep 0.1
done
if [[ ! -s $ready ]] ||
    ! jq -e --argjson port "$port" \
        '.host == "127.0.0.1" and .port == $port and (.pid > 0)' \
        "$ready" >/dev/null; then
    echo "error: local Store origin did not become ready" >&2
    exit 1
fi

"$cloudflared_bin" tunnel --url "http://127.0.0.1:$port" --no-autoupdate \
    >"$tunnel_log" 2>&1 &
tunnel_pid=$!
public_url=
for attempt in $(seq 1 300); do
    public_url=$(awk '
        match($0, /https:\/\/[a-zA-Z0-9-]*\.trycloudflare\.com/) {
            print substr($0, RSTART, RLENGTH)
            exit
        }
    ' "$tunnel_log")
    [[ -n $public_url ]] && break
    if ! kill -0 "$tunnel_pid" 2>/dev/null; then
        cat "$tunnel_log" >&2
        exit 1
    fi
    sleep 0.1
done
if [[ -z $public_url ]]; then
    echo "error: cloudflared did not publish a quick-tunnel URL" >&2
    exit 1
fi

published=$(date +%s)
CP0_TEST_STORE_PAD_BYTES=8388608 \
    "$repo_root/scripts/build-test-store.sh" \
    "$public_url" "$published" "$lifetime"
expires=$((published + lifetime))
jq -n \
    --arg public_url "$public_url" \
    --arg control "$control" \
    --arg origin_log "$origin_log" \
    --arg tunnel_log "$tunnel_log" \
    --argjson origin_pid "$origin_pid" \
    --argjson tunnel_pid "$tunnel_pid" \
    --argjson published "$published" \
    --argjson expires "$expires" \
    '{
      schema_version: 1,
      public_url: $public_url,
      control_file: $control,
      origin_log: $origin_log,
      tunnel_log: $tunnel_log,
      origin_pid: $origin_pid,
      tunnel_pid: $tunnel_pid,
      published_unix_seconds: $published,
      expires_unix_seconds: $expires
    }' >"$runtime"
chmod 0600 "$runtime"

printf 'public Store URL: %s\n' "$public_url"
printf 'runtime evidence: %s\n' "$run_dir"
printf 'v1 throttled: node %q set %q v1-slow %q\n' \
    "$repo_root/scripts/test-store-origin.mjs" "$control" "$rate"
printf 'v2 online:    node %q set %q v2\n' \
    "$repo_root/scripts/test-store-origin.mjs" "$control"
printf 'v2 offline:   node %q set %q offline-v2\n' \
    "$repo_root/scripts/test-store-origin.mjs" "$control"
printf 'Store origin remains active until this process is stopped.\n'
wait "$tunnel_pid"
