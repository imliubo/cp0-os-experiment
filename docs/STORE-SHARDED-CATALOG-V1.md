# Store Sharded Catalog v1

S6E extends Store discovery beyond the legacy 64-application Catalog without
changing the legacy signed bytes or signature domain. A generation containing
at most 64 applications continues to publish the v1-v4 `SignedCatalog` at
`catalog.json`. A larger generation publishes a `SignedCatalogIndex` at that
same path and immutable signed shards below `shards/`.

## Bounds and compatibility

- a legacy Catalog contains at most 64 applications and 48 KiB;
- a root index contains 1-16 descriptors and is at most 48 KiB;
- every shard contains 1-64 applications and is at most 48 KiB;
- a sharded generation contains 1-1024 applications in total, but is used below
  65 applications only when their legacy signed encoding would exceed 48 KiB;
- applications and shard ranges are strictly ordered by App ID;
- devices distinguish the two top-level documents by the mutually exclusive
  `catalog` and `catalog_index` members and reject mixed or unknown fields.

Catalog, root-index and shard signatures use separate domain separators. The
root signature binds the generation sequence and validity window, the content
schema used by every application, total count, editorial projection, exact
category index and every shard URL, SHA-256, byte length, count and App ID
range. Each shard is independently signed and repeats the root sequence,
content schema and ordinal.

## Category index

The root contains one entry for every non-empty signed Store category. Entries
and their shard ordinals are strictly sorted. Each entry binds the exact
application count and the shards containing that category. A device accepts a
generation only after recomputing this index from all verified applications.
The CDN therefore cannot add, remove, move or recategorize an application.

## Publication and device commit

Publisher writes every shard, the root, transparency objects and the public key
to a temporary immutable generation, syncs it, and renames the whole directory.
The database transaction records the root snapshot and every shard before a job
can complete. Startup recovery verifies the root and exact shard set before
repairing `current`.

`cp0-stored` downloads and verifies the root first, then downloads each shard
sequentially. It stages the complete generation in a private directory and
atomically installs that directory before replacing its cached root. A failed,
missing, reordered, substituted or oversized shard leaves the prior generation
active. Search and category browsing operate only on the complete verified
set; installation authorization remains bound to the root sequence and exact
application object.

The transparency log continues to commit `catalog.json`. For a sharded
generation that object is the signed root whose descriptors transitively bind
all shard bytes.
