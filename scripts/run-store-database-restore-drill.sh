#!/usr/bin/env bash
set -euo pipefail
umask 077

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
source_url=${CP0_STORE_DRILL_SOURCE_DATABASE_URL:?CP0_STORE_DRILL_SOURCE_DATABASE_URL is required}
maintenance_url=${CP0_STORE_DRILL_MAINTENANCE_DATABASE_URL:?CP0_STORE_DRILL_MAINTENANCE_DATABASE_URL is required}
restore_url=${CP0_STORE_DRILL_RESTORE_DATABASE_URL:?CP0_STORE_DRILL_RESTORE_DATABASE_URL is required}
restore_database=${CP0_STORE_DRILL_RESTORE_DATABASE:?CP0_STORE_DRILL_RESTORE_DATABASE is required}
evidence_dir=${CP0_STORE_DRILL_EVIDENCE_DIR:-"$repo_root/target/store-resilience/$(date -u +%Y%m%dT%H%M%SZ)-$$"}

if [[ ! $restore_database =~ ^cp0_store_[a-z0-9_]*restore[a-z0-9_]*$ ]]; then
    echo "error: restore database must be a lowercase cp0_store_*restore* test name" >&2
    exit 2
fi
if [[ $source_url == "$restore_url" ]]; then
    echo "error: source and restore database URLs must differ" >&2
    exit 2
fi
case "$evidence_dir" in
    "$repo_root"/target/store-resilience/*) ;;
    *)
        echo "error: drill evidence must stay under target/store-resilience" >&2
        exit 2
        ;;
esac
for command in psql pg_dump pg_restore createdb jq shasum; do
    command -v "$command" >/dev/null || {
        echo "error: required command is unavailable: $command" >&2
        exit 2
    }
done

source_database=$(psql "$source_url" -XAtq --set=ON_ERROR_STOP=1 \
    -c 'SELECT current_database()')
if [[ ! $source_database =~ ^cp0_store_.*(test|drill|publisher).*$ ]]; then
    echo "error: source must be an explicitly named Store test database" >&2
    exit 2
fi
existing=$(psql "$maintenance_url" -XAtq --set=ON_ERROR_STOP=1 \
    -c "SELECT COUNT(*) FROM pg_database WHERE datname = '$restore_database'")
if [[ $existing != 0 ]]; then
    echo "error: restore database already exists; this drill never overwrites or drops it" >&2
    exit 2
fi

mkdir -p "$evidence_dir"
dump_path="$evidence_dir/database.dump"
pg_dump --format=custom --file="$dump_path" "$source_url"
createdb --maintenance-db="$maintenance_url" "$restore_database"
actual_restore_database=$(psql "$restore_url" -XAtq --set=ON_ERROR_STOP=1 \
    -c 'SELECT current_database()')
if [[ $actual_restore_database != "$restore_database" ]]; then
    echo "error: restore URL does not select the newly created database" >&2
    exit 2
fi
pg_restore --exit-on-error --dbname="$restore_url" "$dump_path"

database_fingerprint() {
    psql "$1" -XAtq --set=ON_ERROR_STOP=1 <<'SQL'
SELECT 'audit', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY sequence)) FROM audit_events t
UNION ALL SELECT 'checkpoints', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY tree_size)) FROM store_transparency_checkpoints t
UNION ALL SELECT 'jobs', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY event_id)) FROM store_publication_jobs t
UNION ALL SELECT 'leaves', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY tree_index)) FROM store_transparency_leaves t
UNION ALL SELECT 'migrations', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY version)) FROM _sqlx_migrations t
UNION ALL SELECT 'outbox', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY event_id)) FROM outbox_events t
UNION ALL SELECT 'shards', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY catalog_sequence, shard_index)) FROM store_catalog_shards t
UNION ALL SELECT 'snapshots', COUNT(*), md5(string_agg(row_to_json(t)::text, E'\n' ORDER BY sequence)) FROM store_catalog_snapshots t
ORDER BY 1;
SQL
}

source_fingerprint=$(database_fingerprint "$source_url")
restore_fingerprint=$(database_fingerprint "$restore_url")
if [[ $source_fingerprint != "$restore_fingerprint" ]]; then
    echo "error: restored Store database fingerprint differs from its source" >&2
    exit 1
fi

psql "$restore_url" -Xq --set=ON_ERROR_STOP=1 <<'SQL'
DO $block$
BEGIN
    BEGIN
        UPDATE store_catalog_snapshots SET app_count = app_count WHERE sequence = 1;
        RAISE EXCEPTION USING MESSAGE = $message$append-only mutation accepted$message$;
    EXCEPTION WHEN object_not_in_prerequisite_state THEN
        NULL;
    END;
END
$block$;
SQL

dump_sha256=$(shasum -a 256 "$dump_path" | awk '{print $1}')
jq -n \
    --arg completed_unix_seconds "$(date -u +%s)" \
    --arg source_database "$source_database" \
    --arg restore_database "$restore_database" \
    --arg dump_sha256 "$dump_sha256" \
    --arg fingerprints "$source_fingerprint" \
    '{
        schema_version: 1,
        completed_unix_seconds: ($completed_unix_seconds | tonumber),
        source_database: $source_database,
        restore_database: $restore_database,
        dump_sha256: $dump_sha256,
        table_fingerprints: ($fingerprints | split("\n")),
        append_only_probe: "rejected",
        target_preserved_for_inspection: true
    }' >"$evidence_dir/evidence.json"

echo "Store database restore drill evidence: $evidence_dir/evidence.json"
