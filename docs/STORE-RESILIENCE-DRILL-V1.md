# Store resilience drill v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-RESILIENCE-DRILL-V1.zh-CN.md)

S8E combines capacity, publication, CDN, database recovery, queue replay and
signer failure gates. These are engineering acceptance drills, not a claim
that production multi-region infrastructure or an HSM ceremony is complete.

## Automated gates

- Publisher constructs and independently verifies a 1024-application rich
  Catalog using exactly 16 signed shards. Every shard remains at or below 64
  applications and 48 KiB.
- The loopback Store origin exercises HTTP Range, throttling, generation
  switching, unavailable origin, path escape rejection and removal of an old
  package. Device tests retain the previous Catalog when a shard is missing
  and reject truncated, replaced or digest-mismatched resources.
- PostgreSQL Publisher acceptance injects a Catalog outbox failure, recovers an
  expired lease without reusing a sequence, replays the immutable generation,
  repairs a stale `current` pointer only after full verification, and rejects
  direct SQL mutation.
- The file-key reference signer rejects a missing file, symbolic link, exposed
  mode, wrong length and relative path. Production HSM outage and key ceremony
  remain separate deployment gates.

`scripts/run-store-database-restore-drill.sh` creates a custom-format dump,
refuses to overwrite or drop any database, restores only into an explicitly
named `cp0_store_*restore*` target, compares deterministic row fingerprints for
migrations, publication jobs, Catalog roots/shards, transparency, outbox and
audit, and probes an append-only trigger. It writes private evidence under
`target/store-resilience` and deliberately preserves the restored database for
inspection.

## 2026-08-01 evidence

The PostgreSQL 17 source `cp0_store_s8a_publisher_20260801` was restored into
the new database `cp0_store_s8e_restore_20260801`. Source and restore matched:

| Table | Rows | Row fingerprint |
| --- | ---: | --- |
| `audit_events` | 130 | `c3491b7296cbafd097a9227383576639` |
| `store_transparency_checkpoints` | 65 | `a7614be8c4d2568a035d538a164ca973` |
| `store_transparency_leaves` | 65 | `49cad105d5ba0cfd05d9e8a99992808b` |
| `outbox_events` | 195 | `ecfd20094a86136ff71f85ea4a4ccdec` |
| `store_catalog_shards` | 40 | `22ab8fe508255604ae3c00aa734cf266` |
| `store_catalog_snapshots` | 65 | `4a362f0ed7a47388d69bd4efaf170c81` |

The restored database reported 20 migrations, 61 non-internal triggers and
422 validated constraints. A direct Catalog snapshot update raised SQLSTATE
`55000` and was caught by the drill. Row fingerprints are equality checks for
the restore operation; signed Catalog objects and transparency verification
remain the security integrity mechanism.
