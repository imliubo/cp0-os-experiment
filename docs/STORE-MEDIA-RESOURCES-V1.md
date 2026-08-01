# Store Media Resources v1

S6B defines and publishes the immutable media objects used by Store browsing and
application details. It keeps the signed root Catalog bounded for a 512 MB CM0
while allowing screenshots to be fetched and verified on demand.

## Signed hierarchy

Catalog v3 requires both the v2 `discovery` object and a `resources` object:

```text
Store-signed Catalog v3
  -> icon URL + SHA-256 + bytes + dimensions
  -> details URL + SHA-256 + bytes
       -> description and release notes
       -> screenshot URL + SHA-256 + bytes + dimensions
```

The details document uses `StoreAppDetails` schema v1, rejects unknown fields
and is capped at 16 KiB. It repeats `app_id` and `version`; a device consumer must
match both against the Catalog entry after verifying the details SHA-256.

Catalog v1, v2 and v3 remain distinct. A v1 entry cannot contain discovery or
resources, v2 requires discovery without resources, and v3 requires both. The
Publisher selects the highest schema supported by every projected artifact and
strips newer fields when a legacy artifact requires a lower schema. Mixed schema
entries are never signed.

## Image contract

All images are PNG files whose structure, CRC, dimensions and approved digest
were checked by the isolated Scanner before review:

| Resource | Dimensions | Per-file maximum | Count |
| --- | --- | --- | --- |
| App icon | 48x48 for Listing v1; protocol also reserves 32x32 | 64 KiB | one |
| Screenshot | 320x170 | 512 KiB | one to five |

Every descriptor contains a bounded HTTPS URL, lowercase SHA-256, exact byte
length, width and height. Redirected or substituted CDN content cannot satisfy
the signed descriptor.

## Immutable origin layout

The Publisher derives every path from server IDs and array indices, never from a
developer asset path:

```text
generations/<sequence>/assets/<release-id>/icon.png
generations/<sequence>/assets/<release-id>/details.json
generations/<sequence>/assets/<release-id>/screenshots/<index>.png
```

The Publisher re-reads all approved content-addressed upload chunks. Package,
icon, screenshots, details, Catalog, transparency leaf/checkpoint and public key
are written into one temporary generation, synced, renamed and verified before
the database commit and `current` switch. Later Catalog snapshots keep URLs to
the original immutable generation.

## CM0 cache budget

The device cache implementation must enforce these independent disk budgets:

- verified Catalog: one active file, at most 48 KiB;
- eager icon cache: at most 4 MiB, enough for 64 maximum-size icons;
- details cache: at most 1 MiB, enough for 64 maximum-size manifests;
- on-demand screenshot cache: at most 8 MiB total with verified LRU eviction;
- one temporary resource download at a time; temporary and final bytes count
  against the relevant budget.

Resources are stored by SHA-256, written with owner-only permissions and renamed
only after exact length and digest verification. A missing or corrupt resource
may remove media from Store browsing, but must never block launching an already
installed application.

S6C implements this contract in `cp0-stored` under:

```text
/var/lib/cardputerzero/store/media/icons/<sha256>.png
/var/lib/cardputerzero/store/media/details/<sha256>.json
/var/lib/cardputerzero/store/media/screenshots/<sha256>.png
```

Catalog refresh commits the verified Catalog before best-effort sequential icon
prefetch, so a CDN media failure cannot roll back discovery or block package
installation. Details are decoded again and must match the Catalog `app_id` and
version. Screenshots are fetched on demand and use file modification time for a
stable oldest-first LRU. Startup removes unreferenced icon/details objects,
rejects symlinks and invalid file modes, rechecks retained icon/details objects,
cleans interrupted temporary files without following them, and rechecks each
screenshot when accessed. S6D exposes verified media to the System Shell through
one read-only descriptor bound to strict response metadata; private cache paths
never leave `cp0-stored`. See `STORE-DEVICE-RICH-DETAILS-V1.md`.

## Verification

```sh
cargo test -p cp0-store-protocol -p cp0-store-publisher --lib
cargo test -p cp0-stored --lib

CP0_STORE_TEST_DATABASE_URL=postgres://... \
  cargo test -p cp0-store-publisher --test postgres -- --ignored --nocapture
```

Protocol coverage rejects schema mixing, missing resource descriptors, unsafe
URLs, bad digests, wrong dimensions, duplicated screenshots, unbounded details
and unsafe prose. Publisher unit tests cover v1/v2/v3 projection migration.
Device tests cover exact media caching, owner-only modes, tampered CDN bytes,
Catalog/install independence and bounded screenshot LRU. The PostgreSQL gate
reads every generated object back from the immutable origin, recomputes hashes,
decodes details and proves byte-identical generation reuse after a database
rollback and owner Team rename.
