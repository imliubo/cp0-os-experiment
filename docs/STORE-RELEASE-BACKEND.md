# Store Release Backend

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-RELEASE-BACKEND.zh-CN.md)

S5F adds the developer-facing PostgreSQL and HTTP control path for a reviewed
Store version. It runs outside the device image and does not hold Store signing
keys.

## API slice

- `POST /v1/releases` creates one immutable Release identity from an approved
  Submission.
- `GET /v1/releases/{release_id}` reads a Release owned by the caller's team.
- `POST /v1/releases/{release_id}:schedule` records a future publication time.
- `POST /v1/releases/{release_id}:publish` queues isolated publication and
  returns `202` with state `publishing`.
- `POST /v1/releases/{release_id}:pause|resume|remove` records the developer
  control decision and requests a higher Catalog rebuild.

All writes require `Idempotency-Key`. Existing-resource mutations also require
a strong `If-Match` ETag. Schedule and remove accept strict bounded JSON;
publish, pause and resume require an empty body. Exact retries return the stored
status, body and ETag.

## Authorization and ownership

Reads and writes require a live team member whose current role is `owner` or
`release-manager` and whose token has `store.release` or internal
`store.control`. Writes additionally require current 2FA. The service joins the
Release or Submission through its permanent App owner, so a cross-team ID is
returned as `not-found` rather than exposing its existence.

Creation locks the Submission and accepts only final `approved`. A database
trigger independently requires completed primary and secondary assignments,
two approval decisions and two distinct reviewer identities. Merely writing an
`approved` state cannot create a Release. The database unique constraint allows
exactly one Release identity per immutable Submission, even under concurrent
requests. Rollout percentage and Release identity cannot be changed after
creation.

## State and transaction boundary

Developer-controlled transitions are:

```text
ready          -> scheduled | publishing | removed
scheduled      -> publishing | removed
publish-failed -> publishing | removed
published      -> paused | removed
paused         -> published | removed
```

Only the isolated Publisher may complete `publishing` as `published` or
`publish-failed`. A successful completion must bind a nonzero, globally ordered
Catalog sequence. S5F does not expose an HTTP shortcut for that internal trust
boundary.

Each write uses one PostgreSQL `SERIALIZABLE` transaction containing live
authentication, owner-team lookup, idempotency reservation, row lock, ETag and
state validation, resource update, audit event and outbox event. State changes
also append `release_operations`; database triggers reject update or deletion
of those records and reject skipped versions, mutable rollout or illegal state
metadata.

Removal notes are retained in the append-only operation details for authorized
operators. Outbox payloads carry only the structured reason code, never the
full note. Pause, resume and remove emit `catalog.rebuild-requested`; this event
does not by itself claim that a new signed Catalog exists.

## Verification

```sh
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The PostgreSQL gate covers independent double-approved creation, direct-state
bypass rejection, live role/scope/2FA checks,
cross-team hiding, exact replay, concurrent uniqueness, future-only schedules,
stale ETags, publish queue semantics, simulated Publisher completion,
pause/resume/removal, publish-failed retry, sanitized outbox payloads,
append-only operations, illegal SQL transitions and audit/outbox atomicity.

S5G now implements this trust boundary in `cp0-store-publisher`: reaching
`publishing` only queues work, while the isolated process revalidates immutable
content, signs the package and Catalog, persists the snapshot and then commits
`published`. S5H atomically binds each committed snapshot to an append-only
transparency leaf and signed checkpoint. See `STORE-PUBLISHER.md` and
`STORE-TRANSPARENCY.md`. Production HSM integration and key ceremony remain
external infrastructure gates.
