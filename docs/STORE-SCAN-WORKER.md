# Store Scan Worker

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-SCAN-WORKER.zh-CN.md)

`cp0-store-scan-worker` consumes finalized Submission events and advances only
verified content from `processing` to `ready-for-review`. It is a backend
service and is never included in the CardputerZero device image.

## Trust boundary

The worker does not execute submitted WebAssembly. `cp0-store-scan` performs a
bounded structural parse and validates the exact developer-signed `.capp`,
manifest, SDK version, host imports, declared permissions, Store Listing and
PNG assets. The Listing default locale must match the permanent App Registry.
The package signing key and recomputed fingerprint must match a currently active
key owned by the App team. Key revocation is one-way in PostgreSQL.

The object reader accepts only database-owned lowercase SHA-256 identifiers. It
opens chunk files read-only with `O_NOFOLLOW`, verifies the file type and size,
rehashes each chunk, reconstructs contiguous parts, and then verifies each whole
part and the finalized Submission content digest. Scanner findings are stable
codes and are capped at 16; parser text and filesystem paths are never persisted
in reports. A successful report also carries the versioned deterministic risk
assessment defined in `STORE-RISK-POLICY.md`.

## Delivery and recovery

Finalization writes `submission.scan-requested` to the transaction outbox. A
worker moves one event into `submission_scan_jobs`, marks the event dispatched,
and claims the job with a 60-second lease. Scanning occurs without holding a
database transaction. A short serializable transaction then locks the lease and
Submission, inserts one append-only result and its risk assessment, advances the
resource version, and atomically writes audit and `submission.scan-completed`
outbox records.

Expired leases are returned to the queue unless the eighth claim was exhausted,
in which case the job becomes `failed`. Object or commit failures are also
retried up to eight claims; failure leaves the Submission in `processing` for
operator repair. A unique event and Submission result prevents two workers from
completing the same scan.

## Isolation profile

The reference unit is
`crates/cp0-store-scan-worker/systemd/cp0-store-scan-worker.service`. It runs under a dedicated
account with a read-only object root, no devices, no IP network namespace, no
privileges, no executable writable memory, bounded tasks/CPU/memory, and only
`AF_UNIX`. PostgreSQL must therefore be exposed through a local Unix socket,
with a database role limited to the worker tables and required Submission,
object descriptor, audit and outbox statements.

The environment file requires:

- `CP0_STORE_DATABASE_URL` using the PostgreSQL Unix socket;
- `CP0_STORE_OBJECT_ROOT`, matching the control server object root;
- `CP0_STORE_SCAN_WORKER_ID`, a stable 3-64 byte service identity.

`CP0_STORE_SCAN_ONCE=1` performs at most one poll for controlled jobs and tests.
The default process polls every 500 milliseconds and supports `SIGINT` shutdown.

## Verification

```sh
cargo test -p cp0-store-scan -p cp0-store-scan-worker
cargo clippy -p cp0-store-scan -p cp0-store-scan-worker --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The database gate covers exact single completion, concurrent worker claims,
expired lease recovery, active/revoked developer keys, missing objects, bounded
retry exhaustion, append-only results, immutable jobs and control-server
migration compatibility.

This slice does not provide dynamic malware signatures, external reputation
services, production queue infrastructure, reviewer decisions, Store signing,
Catalog publication or transparency logging.
