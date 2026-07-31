#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
origin="$repo_root/scripts/test-store-origin.mjs"
runner="$repo_root/scripts/run-test-store-origin.sh"
store="$repo_root/target/test-store"
runtime="$repo_root/target/test-store-origin-test/$$"
control="$runtime/control.json"
ready="$runtime/ready.json"
log="$runtime/origin.log"
mkdir -p "$runtime"

node --check "$origin"
bash -n "$runner"
grep -q 'host: "127.0.0.1"' "$origin"
grep -q '"$cloudflared_bin" tunnel --url "http://127.0.0.1:' "$runner"
grep -q 'CP0_TEST_STORE_PAD_BYTES=8388608' "$runner"
if grep -q '0\.0\.0\.0' "$origin" "$runner"; then
    echo "error: Store acceptance origin must remain loopback-only" >&2
    exit 1
fi
if ! jq -e '
    .sequence_v1 == 18000000001 and
    .sequence_v2 == 18000000002 and
    .package_padding_bytes == 65536
' "$store/acceptance.json" >/dev/null 2>&1; then
    CP0_TEST_STORE_PAD_BYTES=65536 \
        "$repo_root/scripts/build-test-store.sh" \
        https://store.example.com/cardputerzero-acceptance \
        1800000000 600 >/dev/null
fi

make_fake_cloudflared() {
    local path=$1
    mkdir -p "$(dirname "$path")"
    printf '#!/bin/sh\nexit 0\n' >"$path"
    chmod 0755 "$path"
}

local_fixture="$runtime/local-fixture"
mkdir -p "$local_fixture/scripts"
local_bin="$local_fixture/target/tools/cloudflared-2026.7.3/source/cloudflared"
env_bin="$runtime/env/cloudflared"
path_bin="$runtime/path/cloudflared"
make_fake_cloudflared "$local_bin"
make_fake_cloudflared "$env_bin"
make_fake_cloudflared "$path_bin"
local_sha=$(shasum -a 256 "$local_bin" | awk '{print $1}')
source_sha=0a59c7b61dedf9096d3df3ee52c7cef81ab31614e8fc8457e864506eae7aa672
sed "s/$source_sha/$local_sha/" "$runner" \
    >"$local_fixture/scripts/run-test-store-origin.sh"
selected=$(PATH="$(dirname "$path_bin"):/usr/bin:/bin" \
    CP0_CLOUDFLARED="$env_bin" \
    /bin/bash "$local_fixture/scripts/run-test-store-origin.sh" \
    --print-cloudflared)
test "$selected" = "$local_bin"

mismatch_fixture="$runtime/mismatch-fixture"
mkdir -p "$mismatch_fixture/scripts"
cp "$runner" "$mismatch_fixture/scripts/run-test-store-origin.sh"
make_fake_cloudflared \
    "$mismatch_fixture/target/tools/cloudflared-2026.7.3/source/cloudflared"
if CP0_CLOUDFLARED="$env_bin" \
    /bin/bash "$mismatch_fixture/scripts/run-test-store-origin.sh" \
    --print-cloudflared >/dev/null 2>&1; then
    echo "error: unverified repository-local cloudflared was accepted" >&2
    exit 1
fi

env_fixture="$runtime/env-fixture"
mkdir -p "$env_fixture/scripts"
cp "$runner" "$env_fixture/scripts/run-test-store-origin.sh"
selected=$(PATH="$(dirname "$path_bin"):/usr/bin:/bin" \
    CP0_CLOUDFLARED="$env_bin" \
    /bin/bash "$env_fixture/scripts/run-test-store-origin.sh" \
    --print-cloudflared)
test "$selected" = "$env_bin"

path_fixture="$runtime/path-fixture"
mkdir -p "$path_fixture/scripts"
cp "$runner" "$path_fixture/scripts/run-test-store-origin.sh"
selected=$(PATH="$(dirname "$path_bin"):/usr/bin:/bin" \
    /bin/bash "$path_fixture/scripts/run-test-store-origin.sh" \
    --print-cloudflared)
test "$selected" = "$path_bin"

if CP0_CLOUDFLARED=relative/cloudflared \
    /bin/bash "$env_fixture/scripts/run-test-store-origin.sh" \
    --print-cloudflared >/dev/null 2>&1; then
    echo "error: relative CP0_CLOUDFLARED path was accepted" >&2
    exit 1
fi

node "$origin" set "$control" v1-slow 32768 >/dev/null
CP0_TEST_STORE_READY_FILE="$ready" \
    node "$origin" serve "$store" "$control" 0 >"$log" 2>&1 &
origin_pid=$!
cleanup() {
    if [[ -n ${origin_pid:-} ]]; then
        kill "$origin_pid" 2>/dev/null || :
        wait "$origin_pid" 2>/dev/null || :
    fi
}
trap cleanup EXIT

for attempt in $(seq 1 100); do
    [[ -s $ready ]] && break
    if ! kill -0 "$origin_pid" 2>/dev/null; then
        cat "$log" >&2
        exit 1
    fi
    sleep 0.05
done
jq -e '
    .schema_version == 1 and
    .host == "127.0.0.1" and
    (.port > 0 and .port <= 65535) and
    (.pid > 0)
' "$ready" >/dev/null
port=$(jq -r .port "$ready")
base_url="http://127.0.0.1:$port"

curl -fsS "$base_url/catalog.json" >"$runtime/catalog-v1.json"
jq -e '
    .catalog.sequence == 18000000001 and
    .catalog.apps[0].version == "1.0.0"
' "$runtime/catalog-v1.json" >/dev/null

package_v1="$store/catalog-v1/apps/dev.cardputerzero.store-test/1.0.0.capp"
started=$(date +%s)
curl -fsS -D "$runtime/range.headers" \
    -H 'Range: bytes=1024-' \
    "$base_url/apps/dev.cardputerzero.store-test/1.0.0.capp" \
    >"$runtime/range.body"
finished=$(date +%s)
if ((finished - started < 1)); then
    echo "error: throttled package response completed without a measurable delay" >&2
    exit 1
fi
grep -qE '^HTTP/[^ ]+ 206' "$runtime/range.headers"
grep -qiE '^Content-Range: bytes 1024-[0-9]+/[0-9]+' \
    "$runtime/range.headers"
tail -c +1025 "$package_v1" | cmp - "$runtime/range.body"

invalid_status=$(curl -sS -o /dev/null -w '%{http_code}' \
    -H 'Range: bytes=999999999-' \
    "$base_url/apps/dev.cardputerzero.store-test/1.0.0.capp")
test "$invalid_status" = 416
secret_status=$(curl -sS -o /dev/null -w '%{http_code}' \
    "$base_url/developer.key")
test "$secret_status" = 404
escape_status=$(curl --path-as-is -sS -o /dev/null -w '%{http_code}' \
    "$base_url/../developer.key")
test "$escape_status" = 404

node "$origin" set "$control" v2 >/dev/null
curl -fsS "$base_url/catalog.json" >"$runtime/catalog-v2.json"
jq -e '
    .catalog.sequence == 18000000002 and
    .catalog.apps[0].version == "1.1.0"
' "$runtime/catalog-v2.json" >/dev/null
old_status=$(curl -sS -o /dev/null -w '%{http_code}' \
    "$base_url/apps/dev.cardputerzero.store-test/1.0.0.capp")
test "$old_status" = 404
curl -fsSI "$base_url/apps/dev.cardputerzero.store-test/1.1.0.capp" \
    >"$runtime/v2-head.headers"
grep -qE '^HTTP/[^ ]+ 200' "$runtime/v2-head.headers"

node "$origin" set "$control" offline-v2 >/dev/null
offline_status=$(curl -sS -o "$runtime/offline.body" -w '%{http_code}' \
    "$base_url/catalog.json")
test "$offline_status" = 503
grep -qx 'test origin offline' "$runtime/offline.body"
node "$origin" status "$control" >"$runtime/status.json"
jq -e '
    .schema_version == 1 and
    .release == "v2" and
    .online == false and
    .package_bytes_per_second == 0
' "$runtime/status.json" >/dev/null

grep -q '"status":206' "$log"
grep -q '"release":"v2"' "$log"

kill "$origin_pid"
wait "$origin_pid"
origin_pid=
