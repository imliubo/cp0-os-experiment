# Store HSM key ceremony evidence

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-HSM-KEY-CEREMONY.zh-CN.md)

The production Store signing key must be generated, rotated, revoked and
destroyed inside an HSM boundary. This document defines the provider-neutral
evidence contract. It does not authorize a production ceremony or select an HSM
vendor, quorum policy, legal retention period or production operator.

## Roles and separation

Every evidence record contains two to four distinct opaque operator IDs and
must include both a `key-custodian` and a `security-officer`. A
`release-operator` or independent `auditor` may also approve, but cannot replace
either required role. Portal, Review Console and Publisher service identities
are not ceremony actors.

The evidence contains only public key IDs and SHA-256 commitments. Private key
bytes, recovery shares, PINs, HSM credentials, personal names, email addresses
and free-form notes are outside the schema and therefore rejected. Detailed HSM
logs remain in the restricted audit system and are bound by
`hsm_attestation_sha256`.

## Operations

- `generate` creates a new public key ID and binds the signed OS trust update.
  It has no previous Catalog sequence.
- `rotate` binds distinct old/new key IDs, a strictly increasing Catalog
  sequence transition and the trust update that establishes overlap.
- `revoke` removes a compromised key. With a replacement key it binds a higher
  Catalog sequence; without one it records no after-sequence and Store remains
  unavailable until a new trust update.
- `destroy` is permitted only after retirement. It binds one old key, no new
  key or trust update, and an unchanged last Catalog sequence.

A ceremony is bounded to eight hours. `approved` means the evidence passed the
local structural policy, not that the HSM, fleet rollout, transparency witness
or CDN promotion was independently audited. `aborted` records use the same
strict shape so failed ceremonies cannot become unbounded incident notes.

## Verification

```sh
./scripts/verify-store-key-ceremony.sh EVIDENCE.json
```

The verifier rejects symbolic, empty or larger-than-32-KiB input, unknown
fields, malformed IDs/digests, repeated actors, missing required roles, reused
keys, non-increasing sequences, invalid operation-specific nullability and
ceremonies longer than eight hours. The schema is
`schemas/store-key-ceremony-v1.schema.json`.

Automated mutation coverage runs in `make check`. Production completion still
requires an approved HSM design, actual quorum execution, signed OS trust-root
rollout, offline-device cohort, transparency/CDN verification and independent
review of the retained evidence.
