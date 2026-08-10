# Store Publication Transparency Log

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-TRANSPARENCY.zh-CN.md)

S5H adds a bounded append-only transparency record for every committed Catalog
snapshot. Its purpose is to make publication history independently
recomputable: publishing, pausing, resuming and removing a Release all produce a
leaf, and the newest signed checkpoint commits to every preceding leaf.

This is backend publication infrastructure. It is not part of the device image,
and it does not replace the signed Catalog or `.capp` verification performed by
the device.

## Objects and ordering

`cp0-store-transparency` defines canonical JSON v1 objects. A leaf binds:

- a contiguous zero-based transparency tree index;
- the monotonic Catalog sequence, digest and byte length;
- the Store signing key ID and publication timestamp;
- the source event, Release, job kind and Release state.

Catalog sequences may contain intentional gaps when a reserved publication is
superseded or fails. Such jobs do not create a snapshot or transparency leaf.
Tree indices remain contiguous, so Catalog sequences `1, 2, 4` map to tree
indices `0, 1, 2` without reusing sequence `3`.

Each successful snapshot also creates a checkpoint containing the tree size,
Merkle root, latest Catalog sequence and issue timestamp. The checkpoint is
signed with the isolated Publisher Store key. PostgreSQL stores the exact
canonical leaf and checkpoint bytes together with independently queryable
digests and ordering metadata.

## Hash and signature construction

The tree follows the RFC 6962 split rule: a nontrivial tree splits at the
largest power of two smaller than its leaf count. CardputerZero uses explicit
versioned domains rather than RFC 6962's one-byte prefixes:

```text
leaf_hash = SHA-256(leaf_domain || uint64_be(length) || canonical_leaf_json)
node_hash = SHA-256(node_domain || left_hash || right_hash)
```

The signed checkpoint message similarly includes a checkpoint-specific domain,
the canonical JSON length and the canonical checkpoint bytes. Ed25519 key IDs
use the same convention as Store package and Catalog signing.

All JSON decoders reject unknown fields, noncanonical re-encoding, invalid IDs,
invalid state/job combinations and objects outside their fixed size bounds. A
v1 tree is bounded to 1,000,000 leaves.

## Atomic publication

The Publisher first writes an immutable candidate generation containing:

```text
generations/<catalog-sequence>/catalog.json
generations/<catalog-sequence>/transparency/leaf.json
generations/<catalog-sequence>/transparency/checkpoint.json
generations/<catalog-sequence>/store.pub
```

The package subdirectory is present only for an initial Release publication.
The database transaction then commits the package record, Catalog snapshot,
leaf, checkpoint, Release transition, audit event, outbox event and completed
job together. PostgreSQL rejects out-of-order inserts, mutation or deletion of
leaves/checkpoints, and completion without all three publication records.

`current` is switched only after the committed Catalog, leaf, checkpoint and
public key match the generation byte-for-byte. Startup performs the same check
before repairing `current`. An unreferenced generation left by a database
rollback is never made current and can only be reused when its deterministic
bytes match the retried job exactly.

## Verification model

Publisher startup decodes every leaf, recomputes every leaf digest, cross-checks
it against its Catalog snapshot and source job, verifies every checkpoint
signature against the Store public key, and recomputes every complete tree
prefix. Snapshot, leaf and checkpoint counts must be one-to-one.

The public crate exposes complete-prefix verification for an observer that has
all leaves through the newer checkpoint. Compact consistency proofs, inclusion
proof serving, external witnesses and gossip are not implemented in S5H and
must not be inferred from the signed checkpoint format.

## Upgrade and key limits

Migration `0007_transparency_log.sql` intentionally does not invent history for
Catalog snapshots created before S5H. If snapshots exist without matching
leaves and checkpoints, the Publisher fails closed. Operators must perform a
reviewed backfill that reconstructs and signs every snapshot in sequence order,
or restore a clean pre-publication database, before enabling the new Publisher.

S5H signs checkpoints with the same isolated raw 32-byte file key used by the
reference Store Publisher. Production HSM integration, key ceremonies,
rotation statements, witness deployment and disaster-recovery procedures remain
separate infrastructure gates.

## Verification

```sh
cargo test -p cp0-store-transparency -p cp0-store-publisher
cargo clippy -p cp0-store-transparency -p cp0-store-publisher --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The PostgreSQL suite verifies complete history and sequence gaps, filesystem
recovery, database and generation tamper rejection, append-only SQL guards, and
atomic rollback of Catalog, transparency, Release, audit and outbox state.
