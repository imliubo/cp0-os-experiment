# Store Publisher, Catalog Builder and Transparency Committer

S5G adds `cp0-store-publisher`, the only reference process allowed to read the
Store Ed25519 private key. It consumes Release control-plane events, revalidates
approved immutable content, Store-signs packages, builds a deterministic signed
Catalog and publishes immutable static-origin generations. It is a backend
service and is never included in the CardputerZero device image. S5H extends
the same isolated commit boundary with an append-only transparency leaf and
signed checkpoint for every Catalog snapshot.

## Trust boundary

The control server, Developer Portal and Review Console never receive the
private key. The reference signer accepts one raw 32-byte Ed25519 key from an
absolute, no-follow, regular file with mode `0600` or stricter. This is a tightly
isolated file-key implementation for development and controlled deployments; it
does not claim production HSM integration or replace a production key ceremony.

Before signing, the Publisher reconstructs every Submission part from immutable
content-addressed chunks. It validates chunk continuity, size and SHA-256, each
whole-part digest and the finalized Submission content digest. It then decodes
the developer `.capp`, verifies its signature against a currently active key
owned by the App team, rejects an existing Store signature, and reparses the
manifest and Store Listing. Package, Listing, permanent App registry, Release,
default locale and approved assets must all agree exactly.

S6A makes production snapshots strict Discovery Catalog v2. The Publisher also
reads the owner Team name and approved default Listing localization, then signs
developer, subtitle, category, keywords, age rating and privacy/support URLs
into each entry. Catalog v1 remains accepted for offline recovery fixtures, but
v1/v2 fields cannot be mixed. If an upgraded projection still contains a legacy
artifact, the complete snapshot remains pure v1 until every included artifact
has complete signed discovery data. See `STORE-DISCOVERY-CATALOG-V2.md`.

S6B adds Catalog v3 media descriptors. The Publisher writes fixed server-derived
icon, screenshot and details paths in the same immutable generation as the
Store-signed package. The root Catalog binds icon/details hashes and the bounded
details document binds every screenshot. Migration selects pure v1, v2 or v3
for the complete projection; schema fields are never mixed. See
`STORE-MEDIA-RESOURCES-V1.md`.

S6E adds a backward-compatible signed root index. A projection uses the legacy
v1-v4 `SignedCatalog` only while it has at most 64 applications and its encoded
bytes fit the 48 KiB limit. Otherwise the Publisher deterministically packs
applications into independently signed, size-tested shards. The signed root
binds every shard descriptor, the exact category index and any v4 editorial
projection. See `STORE-SHARDED-CATALOG-V1.md`.

## Queue and sequence model

`release.publish-requested` and `catalog.rebuild-requested` outbox records are
materialized as leased `store_publication_jobs`. PostgreSQL verifies that every
job matches its source event. Only one job may be `running`; its first claim
advances the singleton Catalog counter by exactly one and permanently reserves:

- a never-reused global Catalog sequence;
- deterministic publication and expiry timestamps;
- the source Release state and resource version;
- the App owner Team's validated developer display name.

The developer name is frozen atomically on the first claim and cannot be changed
by later SQL updates. Team renames therefore do not change a generation after it
has been written but before its database commit. Retries retain every reserved
input, so they reproduce identical signed bytes. An expired lease is requeued
through attempt seven. Attempt eight, an invalid approved source or a revoked
developer key ends the job. Initial publication atomically advances `publishing`
to `published` or `publish-failed`; Catalog rebuild failure leaves the already
committed Release state unchanged.

A control event superseded before publication consumes its reserved sequence
without creating a snapshot. This intentional gap prevents sequence reuse. The
newest event will build the current state. Audit and outbox records distinguish
published, failed and superseded outcomes.

## Catalog projection

Each successfully Store-signed package has one append-only artifact record and
one immutable path:

```text
generations/<initial-sequence>/packages/<release-id>.capp
```

For each App, the builder selects the Release with the greatest existing package
publication sequence. `published` includes that Release, while `paused` or
`removed` excludes the App without falling back to an older version. A new
`publishing` target temporarily has the reserved sequence and supersedes older
versions while its Catalog is built. Pause, resume and remove each request a new
global snapshot but retain the Release's original package sequence.

Catalog applications are sorted by App ID. A legacy root remains bounded to 64
applications and 48 KiB; an indexed generation is bounded to 16 shards, 64
applications and 48 KiB per shard, and 1024 applications in total. The default
Listing localization supplies the signed name and summary; manifest permissions
are sorted before encoding. PostgreSQL stores the exact signed root and shard
bytes with append-only digest metadata.

## Filesystem commit and recovery

The Publisher cannot atomically commit PostgreSQL and a static filesystem. It
therefore uses this ordering:

1. reserve sequence and timestamps in PostgreSQL;
2. build deterministic signed artifacts;
3. write, sync and atomically rename an immutable generation containing the
   Catalog root, any shards, transparency leaf/checkpoint and Store public key;
4. atomically commit package metadata, Catalog snapshot, transparency records,
   Release state, audit and outbox;
5. verify every committed generation object and atomically switch `current`.

A crash before step 4 leaves an unreferenced generation that is verified and
reused by the same leased job. A crash after step 4 temporarily leaves the prior
Catalog visible; startup verifies the complete database transparency history,
then checks the highest Catalog, every bound shard, leaf, checkpoint and public key byte-for-byte
before repairing `current`. A database with no snapshot rejects any pre-existing
`current` pointer instead of exposing uncommitted content. The process never
rewrites or removes a committed generation.

The transparency protocol, complete-prefix verifier, migration behavior and
explicit limitations are documented in `STORE-TRANSPARENCY.md`.

## Isolation profile

`crates/cp0-store-publisher/systemd/cp0-store-publisher.service` runs as a
dedicated account with no IP network namespace, devices, capabilities, home
access or executable writable memory. It receives only PostgreSQL over a Unix
socket, read-only Submission objects and key, and one writable origin root.
Production database roles must be limited to the publication queue, required
read models, Release completion, immutable artifacts, audit and outbox.

Required environment variables are documented in
`crates/cp0-store-publisher/systemd/store-publisher.env.example`.

## Verification

```sh
cargo test -p cp0-store-publisher
cargo clippy -p cp0-store-publisher --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The PostgreSQL gate covers package, Catalog and checkpoint signatures, exact
immutable paths, concurrent single claim with bounded serialization retry,
monotonic non-reused sequences, complete transparency prefixes, latest-version
projection, pause/resume/remove, stale-event coalescing, permanent failure,
append-only records, SQL bypass attempts, atomic rollback, deterministic reuse
after a Team rename, tamper rejection and crash recovery of `current`.

Production HSM integration, key rotation ceremony, multi-origin/CDN promotion,
disaster recovery, compact consistency proofs and external witnesses remain
separate infrastructure gates.
