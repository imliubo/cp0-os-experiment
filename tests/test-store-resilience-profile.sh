#!/bin/sh
set -eu

repo=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
drill="$repo/scripts/run-store-database-restore-drill.sh"
document="$repo/docs/STORE-RESILIENCE-DRILL-V1.md"

test -x "$drill"
test -f "$document"
bash -n "$drill"
grep -q 'restore database already exists; this drill never overwrites or drops it' "$drill"
grep -q 'target/store-resilience' "$drill"
grep -q 'append-only mutation accepted' "$drill"
grep -q 'target_preserved_for_inspection: true' "$drill"
grep -q 'publisher_verifies_the_maximum_rich_catalog_capacity' \
    "$repo/crates/cp0-store-publisher/src/lib.rs"
grep -q 'signing_key_loader_fails_closed_on_unsafe_or_unavailable_keys' \
    "$repo/crates/cp0-store-publisher/src/lib.rs"
if grep -Eq '(^|[[:space:]])(dropdb|rm)([[:space:]]|$)|DROP DATABASE' "$drill"; then
    echo "error: resilience drill must not delete evidence or databases" >&2
    exit 1
fi
if "$drill" >/dev/null 2>&1; then
    echo "error: resilience drill ran without explicit database configuration" >&2
    exit 1
fi
