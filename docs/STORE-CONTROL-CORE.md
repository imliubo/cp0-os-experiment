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

The current `ControlPlane` keeps state in memory so its transaction semantics
can be tested deterministically. It is not a production persistence substitute.
The next adapter must map one core mutation to one PostgreSQL serializable
transaction containing resource state, idempotency result, audit row and outbox
row. Object bytes remain in immutable object storage and are referenced only by
their declared size and SHA-256.

Production IDs must be allocated by the persistence adapter or an injected
cryptographic ID source. The in-memory counter is deterministic test scaffolding
and must not be exposed by a public deployment.

## Verification

```sh
cargo test -p cp0-store-control
cargo clippy -p cp0-store-control --all-targets -- -D warnings
```
