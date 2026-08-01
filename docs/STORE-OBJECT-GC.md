# Store content object garbage collection

`cp0-store-object-gc` removes unreachable upload chunks and interrupted temporary
files from the local content-addressed reference backend. It does not collect
Publisher generations, device caches, or application packages, and it is not a
substitute for a replicated production object store.

## Safety contract

- no arguments means `--dry-run`; deletion requires the explicit `--apply` flag;
- apply mode refuses a minimum age below 86,400 seconds;
- the object root must already exist at an absolute path with real `chunks` and
  `temporary` directories that grant no group/other access; dry-run never
  creates or changes their permissions and never runs database migrations;
- chunk prefixes, filenames, digests, temporary names, regular-file types,
  inodes, sizes, link counts and modification times are validated before any
  deletion;
- an unexpected file, directory, symbolic link, path escape, changing inode or
  inventory above one million files fails the complete run closed;
- referenced rows in append-only `submission_upload_chunks` are always retained;
- unreferenced chunks and temporary files younger than the configured grace
  period are retained;
- successful deletions fsync the directory that contained each object.

Uploads take a shared PostgreSQL transaction advisory lock immediately before
writing an object and retain it through the database commit. GC scans the
filesystem, then takes the matching exclusive lock before resolving references,
revalidating candidates and deleting. Concurrent uploads can proceed together;
GC either observes their committed reference or completes before they write.
An interrupted apply can be rerun safely because object names are immutable and
each deletion was independently proven unreachable while the exclusive lock was
held.

## Operation

Use the same private database and object root as the control service. Review the
bounded JSON report before applying:

```sh
CP0_STORE_DATABASE_URL=postgres://... \
CP0_STORE_OBJECT_ROOT=/var/lib/cardputerzero-store/objects \
cargo run -p cp0-store-control-server --bin cp0-store-object-gc

CP0_STORE_DATABASE_URL=postgres://... \
CP0_STORE_OBJECT_ROOT=/var/lib/cardputerzero-store/objects \
cargo run -p cp0-store-control-server --bin cp0-store-object-gc -- --apply
```

`--minimum-age-seconds N` can inspect a shorter interval in dry-run or increase
the default 24-hour grace period. Apply mode deliberately cannot reduce it. The
report includes the database observation time plus aggregate counts and bytes
only; it does not print object contents, developer identities, tokens or
database credentials. Schema migrations must be deployed before this separate
maintenance command is run.

## Verification

The PostgreSQL 17 acceptance test creates referenced, orphaned, young and
temporary objects. It proves dry-run makes no changes, the age gate retains new
files, apply removes only unreachable files, referenced content remains readable,
and a symbolic-link entry prevents every deletion in that run. Migration 23 adds
the digest index used by the bounded 512-object reference batches.

Production replication, retention policy approval, deletion evidence retention,
multi-region lifecycle rules and restore coordination remain external
infrastructure gates.
