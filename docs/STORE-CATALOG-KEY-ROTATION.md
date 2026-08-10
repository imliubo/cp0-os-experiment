# Store Catalog key rotation

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-CATALOG-KEY-ROTATION.zh-CN.md)

This runbook defines the engineering boundary for rotating the Store Catalog
signing key. It does not claim that a production HSM, operator quorum, CDN or
fleet orchestration service exists.

## Trust model

`cp0-stored` selects one root-owned public key by the exact lowercase SHA-256
`key_id` carried in a signed Catalog. A Catalog cannot add or remove a trusted
key. Trust-root changes must arrive in a separately signed OS update or image.
The browser Portal, control plane and public CDN cannot modify this directory.

The device trust directory may contain the old and new public keys during a
bounded overlap. Every Catalog and shard generation is still signed by exactly
one key, and Catalog sequence monotonicity continues across the key change.

## Planned rotation

1. Generate the new key inside the production signing boundary. Record its
   algorithm, public key, `key_id`, custody owners and activation window in the
   reviewed ceremony record. Never export the private key.
2. Produce a signed OS trust update that adds `<new-key-id>.pub` without
   removing `<old-key-id>.pub`. Verify root ownership, mode, exact 32-byte
   length and filename-to-key digest binding on a canary device.
3. Keep publishing with the old key until the trust update has reached the
   required fleet threshold and rollback cohort. The last old-key Catalog must
   remain valid for the documented offline grace period.
4. Publish the next, strictly higher Catalog sequence with the new key. Verify
   the generation, transparency checkpoint, CDN bytes and canary refresh before
   promotion. Never reuse a sequence during fallback.
5. After all supported devices have the new key and every old-key Catalog has
   expired, ship another signed OS trust update that removes the old public key.
   Restart `cp0-stored` in the same update so its cached Catalog is revalidated
   against the post-update trust root before serving or authorizing installs.
6. Retain public records, ceremony evidence and old signatures. Retire or
   destroy the old private key according to HSM policy; do not delete Catalog,
   package, audit or transparency history.

An offline device that missed the overlap cannot trust a new-key Catalog. It
must first receive a signed OS update containing the new public key. Catalog or
CDN content is never an acceptable trust bootstrap.

## Emergency revocation

Compromise response is a signed OS security update that removes the affected
public key, restarts `cp0-stored`, and installs an uncompromised replacement
key when available. Until that update is installed, already-running devices can
retain an old verified Catalog in memory through its signed validity window;
publishing a higher sequence alone is not revocation.

The incident operator must preserve the last known-good database, immutable
generation objects, transparency prefix, HSM audit and CDN request evidence.
Restoring an older database or Catalog sequence is forbidden. If no trusted key
remains, Store refresh and install fail closed while locally installed apps
continue to run.

## Verification

```sh
cargo test -p cp0-stored catalog_key_rotation_requires_overlap
cargo test -p cp0-store-protocol -p cp0-store-publisher
cargo clippy -p cp0-stored --all-targets -- -D warnings
```

The rotation test starts on an old-key Catalog, adds the new public key, accepts
a higher new-key Catalog during overlap, removes the old public key, restarts
the service model and rejects a still-higher old-key Catalog without replacing
the cached new-key generation.

Production completion additionally requires an HSM-backed two-person ceremony,
signed OS trust updates, a representative offline-device cohort, CDN promotion
and rollback exercises, and independent audit of the resulting evidence.
