#!/bin/bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
contract="$repo_root/sdk/abi/cardputerzero-hostcalls-v1.json"
snapshots=(
    "$repo_root/sdk/abi/compat/cardputerzero-hostcalls-0.1.json"
    "$repo_root/sdk/abi/compat/cardputerzero-hostcalls-1.0.json"
)

jq -e '.abi_version == "0.1"' "${snapshots[0]}" >/dev/null
jq -e '.abi_version == "1.0"' "${snapshots[1]}" >/dev/null

jq -e '
    .schema_version == 1 and
    .abi_version == "1.0" and
    .module == "cardputerzero" and
    (.imports | length == 28) and
    ([.imports[].name] | length == (unique | length)) and
    ([.imports[].c_name] | length == (unique | length)) and
    ([.imports[].wit] | length == (unique | length))
' "$contract" >/dev/null
for snapshot in "${snapshots[@]}"; do
    jq -e --slurpfile snapshot "$snapshot" '
        . as $current |
        $current.module == $snapshot[0].module and
        ($snapshot[0].imports | length) == 22 and
        all($snapshot[0].imports[]; . as $required |
            any($current.imports[];
                .name == $required.name and
                .wamr_signature == $required.wamr_signature))
    ' "$contract" >/dev/null
done
node "$repo_root/scripts/generate-sdk-bindings.mjs" --check

grep -qx '#define CP0_SDK_VERSION_MAJOR 1' \
    "$repo_root/sdk/c/include/cardputerzero.h"
grep -qx '#define CP0_SDK_VERSION_MINOR 0' \
    "$repo_root/sdk/c/include/cardputerzero.h"
grep -qx 'version = "1.0.0"' "$repo_root/sdk/rust/Cargo.toml"

if rg -n 'link_name = "cp0_' "$repo_root/sdk/rust/src" \
    --glob '!host_imports.rs'; then
    echo "private Rust host imports must come from generated bindings" >&2
    exit 1
fi

registered=$(rg -c '^\{"cp0_' "$repo_root/app-runtime/src/hostcall_symbols.inc")
test "$registered" -eq 28
