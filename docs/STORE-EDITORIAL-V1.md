# Store editorial collections v1

S8A adds an audited control-plane path for the 320x170 Today page and carries
the result to devices in signed Catalog v4. Operators can select content only
from approved, currently published Releases. The device never trusts an
operator API response directly.

## Bounded layout

There is one v1 layout with the stable identity `today`. A complete replacement
contains:

- one 1-48 character headline;
- one featured Release;
- one or two collections with distinct 1-32 character titles;
- one to four Releases in each collection.

Whitespace-only, leading/trailing whitespace, control characters, duplicate
titles, duplicate Release IDs, and duplicate App IDs are rejected. The
featured application cannot also appear in a collection. Every identifier must
resolve to a Release whose state is `published`, whose Submission remains
`approved`, and whose App ID and version still match that Submission.

The response resolves every Release ID to its authoritative App ID. Operators
do not submit App IDs, so they cannot create a Release/App mismatch.

## Operator API

The canonical schema is `schemas/store-control-v1.openapi.json`:

```text
GET  /v1/editorial/today
POST /v1/editorial/today
PUT  /v1/editorial/today
```

GET returns the current layout and ETag. POST creates resource version 1,
requires `Idempotency-Key`, and explicitly rejects `If-Match`; it returns 409
if the layout already exists. PUT replaces the complete layout, requires both
`Idempotency-Key` and the current `If-Match`, and increments the resource
version. A stale ETag returns 412 and a PUT before creation returns 404.

This API uses an isolated Store operator identity and token domain. Access
requires an active `editor` or `admin`, enabled 2FA, a live non-revoked token,
and the exact `store.editorial` scope. Developer, reviewer, and operator tokens
cannot share a digest or cross identity domains.

Each successful write is one serializable transaction containing:

- the current `store_editorial_layouts` row;
- an append-only `store_editorial_revisions` row for the exact new version;
- an `editorial.today-created` or `editorial.today-updated` audit event;
- a `catalog.rebuild-requested` outbox event bound to the featured Release and
  the new `editorial_resource_version`;
- the completed idempotency response.

A deferred database constraint requires the layout, revision, audit event, and
outbox event to agree before commit. Database triggers reject deletion,
non-monotonic versions, unpublishable references, or updates that omit any of
those records. An idempotent replay returns the original body and ETag without
creating another revision or rebuild request.

## Catalog v4 publication

An editorial rebuild job carries the exact immutable
`editorial_resource_version` from its outbox event through the publication job
and Catalog snapshot. The Publisher reloads that revision rather than the
mutable current row. This makes a delayed or retried v1 job reproducible even
after operators create v2.

Catalog v4 extends the signed Catalog with:

```json
{
  "editorial": {
    "headline": "Reviewed for 320 x 170",
    "featured_app_id": "dev.cardputerzero.featured",
    "collections": [
      {
        "title": "Small-screen essentials",
        "app_ids": ["dev.cardputerzero.notes"]
      }
    ]
  }
}
```

The projection uses App IDs only after checking that every referenced Release
still has a published artifact matching its authoritative App ID and version,
and that every App is present in the same Catalog application set. Catalog v4
requires v2 discovery fields, v3 resource fields, and valid editorial metadata;
older schema versions must not contain the `editorial` field.

If a referenced Release is paused, removed, superseded, or absent from the
projected application set, a normal build completes fail-closed as Catalog v3
with no editorial data. Resuming the Release permits a subsequent rebuild to
return to v4. A bound editorial job with a missing immutable revision is a hard
publication error, not a fallback.

Release-driven rebuild jobs do not bind a separate editorial revision. They
read the current valid layout while preparing the snapshot; the signed Catalog
bytes still bind the exact emitted projection, while the snapshot's
`editorial_resource_version` column remains null. The column records explicit
editorial-job provenance, not whether a Catalog contains editorial data.

## Device IPC and UI

`cp0-stored` validates the Catalog signature, schema, collection bounds,
distinct App IDs, and membership in the signed application set. The local
protocol adds a strict request:

```json
{"protocol_version":1,"request_id":7,"command":{"name":"today"}}
```

The response repeats `sequence`, `expires_unix_seconds`, and `stale`, then
returns either `editorial: null` for Catalog v1-v3 or a bounded Today object.
Featured and collection items are full `StoreAppSummary` values derived from
the same verified Catalog, including current install/update operation state.
No URL, Release ID, database field, or unsigned operator text crosses the local
IPC boundary.

The System Shell fetches Today immediately after Catalog and accepts it only
when both responses have the exact same sequence. A null layout, parse error,
IPC error, or mismatched sequence clears all editorial state. Background
Catalog refresh preserves the selected collection and application by stable
title and App ID when they still exist.

On the 320x170 single-foreground UI, Today shows one featured application and
up to two collection rows. Enter opens the featured details or the selected
collection; a collection shows up to four applications. Left/right tab changes
are disabled inside a collection. Escape closes details first, then the
collection, before leaving Store navigation.

## Verification

PostgreSQL acceptance tests cover operator authentication, create/replay/update,
stale ETags, invalid or duplicate references, immutable revisions, direct SQL
tampering, audit/outbox rollback, exact Publisher revision replay, Release
pause/resume/remove fallback, job supersession, and snapshot provenance.
Protocol and daemon tests cover Catalog v1-v4 compatibility and strict Today
projection. System Shell tests cover parsing, navigation, refresh continuity,
operation propagation, and pixel snapshots at exactly 320x170.
