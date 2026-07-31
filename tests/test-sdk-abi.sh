#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
contract="$repo_root/sdk/abi/cardputerzero-hostcalls-v1.json"
snapshot="$repo_root/sdk/abi/compat/cardputerzero-hostcalls-0.1.json"

jq -e '
    .schema_version == 1 and
    .abi_version == "0.1" and
    .module == "cardputerzero" and
    (.imports | length == 22) and
    ([.imports[].name] | length == (unique | length)) and
    ([.imports[].c_name] | length == (unique | length)) and
    ([.imports[].wit] | length == (unique | length))
' "$contract" >/dev/null
jq -e --slurpfile snapshot "$snapshot" '
    . as $current |
    $current.module == $snapshot[0].module and
    all($snapshot[0].imports[]; . as $required |
        any($current.imports[];
            .name == $required.name and
            .wamr_signature == $required.wamr_signature))
' "$contract" >/dev/null
node "$repo_root/scripts/generate-sdk-bindings.mjs" --check

if rg -n 'link_name = "cp0_' "$repo_root/sdk/rust/src" \
    --glob '!host_imports.rs'; then
    echo "private Rust host imports must come from generated bindings" >&2
    exit 1
fi

registered=$(rg -c '^\{"cp0_' "$repo_root/app-runtime/src/hostcall_symbols.inc")
test "$registered" -eq 22
