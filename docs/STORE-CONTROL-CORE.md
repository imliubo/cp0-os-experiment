# Store Control Transaction Core

`cp0-store-control` is the first S5 backend slice. It is a framework-independent
Rust domain core for the frozen Store Control API. It does not listen on a
network port and is not linked into the device image.

The core owns these invariants before any PostgreSQL or HTTP adapter runs:

- team roles come from current server-side membership, and every user write
  requires two-factor authentication;
- a team always retains at least one Owner and member email identities are
  unique inside the team;
- App IDs have one permanent owner and no delete/recycle transition;
- submission package, Listing and asset descriptors are frozen per revision;
- only the scanner and reviewer service roles can perform their exact state
  transitions, and only an approved submission can create a Release;
- mutations require an exact resource version and a bounded idempotency key;
- exact retries return the original result, while reuse with another request
  fails with `idempotency-conflict`;
- every successful mutation appends both a sanitized audit event and an outbox
  event; failed preconditions append neither;
- audit events store hashes of requests and idempotency keys, never the raw
  idempotency key;
- completed Catalog publication sequences are nonzero and globally monotonic.

The `ControlPlane` keeps state in memory so its complete transaction semantics
can be tested deterministically. It is not a production persistence substitute.
The persistence adapter is implemented by `cp0-store-control-server` for App
registration/lookup and the create/upload/finalize/read Submission path. One
mutation maps to one PostgreSQL serializable transaction containing resource
state, idempotency result, audit row and outbox row. Uploaded bytes use an
owner-only content-addressed backend and the database references only declared
sizes, SHA-256 digests and immutable chunk descriptors. Runtime details and
remaining gaps are documented in `STORE-CONTROL-SERVER.md`.

The adapter hashes bearer tokens before lookup and validates token expiry,
revocation, current team role, current 2FA state and scope inside the database
transaction. It accepts only bounded OpenAPI JSON, returns bounded
`application/problem+json`, retries serialization/deadlock failures, and binds
to loopback by default. A non-loopback bind requires an explicit environment
gate and TLS termination outside the process.

The migrations also enforce permanent App ownership, immutable Submission
content and uploaded chunk descriptors, one-time finalize digest, immutable
Release identity, append-only Review/Audit records, at least one team Owner,
stable member identity, and one-way token revocation. The remaining
Submission/Review/Release HTTP operations must reuse these transaction and
response boundaries.

Production IDs must be allocated by the persistence adapter or an injected
cryptographic ID source. The in-memory counter is deterministic test scaffolding
and must not be exposed by a public deployment.

## Verification

```sh
cargo test -p cp0-store-control
cargo clippy -p cp0-store-control --all-targets -- -D warnings
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings
cargo +1.85.1 check -p cp0-store-control-server --all-targets

# Requires a disposable PostgreSQL database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```
