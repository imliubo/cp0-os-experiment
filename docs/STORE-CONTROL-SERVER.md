# Store Control Server

`cp0-store-control-server` is the PostgreSQL and HTTP adapter for the frozen
Store Control API. It is a developer/reviewer control-plane service and is not
part of the CardputerZero device image.

## Implemented API slice

- `POST /oauth/device/code`, `/oauth/device/authorize`, and `/oauth/token`;
- `GET /v1/teams/{team_id}`;
- `POST /v1/teams/{team_id}/members/{member_id}:set-role`;
- `POST /v1/teams/{team_id}/members/{member_id}:remove`;
- `POST /v1/apps` and `GET /v1/apps/{app_id}`;
- `POST /v1/apps/{app_id}/submissions`;
- `PUT /v1/submissions/{submission_id}/parts/{part_name}`;
- `POST /v1/submissions/{submission_id}:finalize`;
- `POST /v1/submissions/{submission_id}:withdraw`;
- `GET /v1/submissions/{submission_id}`;
- `POST /v1/submissions/{submission_id}/messages`;
- `GET /v1/review/submissions`;
- `POST /v1/review/submissions/{submission_id}:begin`;
- `POST /v1/review/submissions/{submission_id}/decisions`;
- `POST /v1/releases` and `GET /v1/releases/{release_id}`;
- `POST /v1/releases/{release_id}:schedule|publish|pause|resume|remove`;
- `POST /reports/v1/content`;
- `GET /v1/moderation/reports` and
  `POST /v1/moderation/reports/{report_id}:decide`;
- `GET /v1/apps/{app_id}/moderation-notices`;
- `POST /v1/moderation/notices/{notice_id}:appeal` and
  `POST /v1/moderation/appeals/{appeal_id}:decide`.

All writes authenticate the hashed bearer token and re-read current team role,
2FA state and scope inside a PostgreSQL `SERIALIZABLE` transaction. App writes
require `store.apps.write`; Submission writes require `store.submit`. The
internal `store.control` scope can perform either operation. Exact idempotent
retries return the stored status/body/ETag, while a key reused for another
request fails with `idempotency-conflict`.

The developer OAuth Device Flow issues only 15-minute `store.submit` tokens.
Device codes and access tokens are stored only as hashes; approval requires a
live owner/developer identity with the exact scope and enabled 2FA. Poll timing,
one-time exchange, approval idempotency, state transitions, audit, and outbox
are enforced transactionally. See `STORE-OAUTH-DEVICE-FLOW.md` for the protocol
and the remaining production Identity/Teams boundary.

Moderation v1 is a non-production engineering slice. Public intake accepts only
an exact published Release and one fixed reason; it requires a random
idempotency key and accepts no free text, account/device identity, contact data,
request timestamp, IP, User-Agent, log, or attachment. An active 2FA `admin`
with the exact `store.moderation` scope can triage the bounded SLA queue. Team
owners/developers can read only their App notices and create one structured
appeal. All transitions are serializable, versioned, idempotent, audited,
outboxed, and backed by append-only revisions. The slice does not alter Release
or Catalog state; production enforcement remains blocked on approved policy,
dual control, reversible suppression, and operations ownership. See
`STORE-MODERATION-V1.md`.

Team reads expose only the caller's bounded active-member list. Role changes
and terminal member removal require an owner with `store.teams.write`, a strong
team ETag, and MFA authenticated within five minutes. The transaction advances
Team/member versions, preserves the last active Owner, revokes the target
member's existing tokens, and emits one audit/outbox event. Removal retains the
identity row for references and audit while excluding it from authentication.
The external OIDC and Portal BFF boundary is frozen in
`STORE-IDENTITY-TEAMS.md`; credentials are outside this service.

The upload endpoint accepts one contiguous chunk of at most 256 KiB and checks
`If-Match`, `Content-Range` and `Content-SHA256`. Chunks are hashed again and
stored under an owner-only `0700` content-addressed root. The database stores
only immutable chunk descriptors. Finalize locks the Submission, reopens every
chunk, recomputes every declared object digest and the frozen Submission content
digest, then atomically changes `uploading` to `processing` and emits the scan
request through the transaction outbox.

Withdraw accepts an empty body and allows an owner/developer to close a
`draft`, `uploading`, `processing`, `ready-for-review`, or `in-review`
Submission. The same transaction advances its ETag, cancels any queued/running
scan job and active review assignment, suppresses an unconsumed scan-requested
event, and appends `submission.withdrawn` to audit/outbox. Files and prior
events remain immutable. See `STORE-SUBMISSION-WITHDRAWAL.md`.

`cp0-store-scan-worker` consumes that event through an expiring database lease.
It independently reopens the read-only objects, binds the package key to an
active team Developer Key, performs bounded package/WASM/Listing/PNG checks,
and atomically records an append-only result before advancing the Submission.
See `STORE-SCAN-WORKER.md` for its separate trust boundary and host profile.

Human review uses a separate internal reviewer identity and token domain; a
reviewer is never represented as an App team member. Queue reads and review
writes require an active identity, 2FA, a live one-hour token, and the exact
`store.review` scope. Begin and decision operations preserve the same
SERIALIZABLE, ETag, idempotency, audit, and outbox guarantees as developer
mutations. See `STORE-REVIEW-BACKEND.md` for assignment and message rules.

Release reads and writes require an owner or release-manager with
`store.release` (or the internal `store.control` scope); writes additionally
require live 2FA. Creation locks and verifies an owner-team approved Submission.
Schedule, publish, pause, resume and remove use strong ETags and append an
immutable operation record in the same transaction as audit and outbox. Publish
only enters `publishing` and emits `release.publish-requested`; an isolated
Publisher must sign and publish a Catalog before setting `published` and a
Catalog sequence. S5G implements that boundary in `cp0-store-publisher`; see
`STORE-RELEASE-BACKEND.md` and `STORE-PUBLISHER.md`.

A database rollback after an object write can leave an unreachable
content-addressed chunk. It grants no Submission state and can be removed by a
future mark-and-sweep maintenance worker. Production replication and garbage
collection remain separate from this local filesystem reference backend.

## Run

The binary requires:

- `CP0_STORE_DATABASE_URL`: PostgreSQL connection URL;
- `CP0_STORE_OBJECT_ROOT`: absolute, service-owned object directory;
- `CP0_STORE_LISTEN_ADDR`: optional, defaults to `127.0.0.1:8787`.

Non-loopback binding is rejected unless `CP0_STORE_ALLOW_NON_LOOPBACK=1` is set.
That gate does not add TLS: a production deployment must terminate HTTPS in a
separate, hardened ingress and keep the service/database/object root private.

```sh
CP0_STORE_DATABASE_URL=postgres://... \
CP0_STORE_OBJECT_ROOT=/var/lib/cardputerzero-store/objects \
cargo run -p cp0-store-control-server
```

## Verification

```sh
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings
cargo +1.85.1 check -p cp0-store-control-server --all-targets

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The database gate covers exact replay, competing App IDs, concurrent Submission
revision allocation and review claims, live RBAC/2FA/scope/revocation checks,
256 KiB chunk boundaries, stale ETags, non-contiguous ranges, digest mismatches,
finalize replay, independent primary/secondary assignment authorization,
structured decisions,
developer/reviewer messages, approved-only Release creation, concurrent Release
uniqueness, scheduling, publication queueing, pause/resume/removal, publication
retry, Submission withdrawal/cleanup/replay, injected transaction rollback and
append-only database triggers, Team isolation, MFA freshness, last-Owner
protection, role-change/removal replay, immediate token revocation, terminal
membership database enforcement, double-approved Release enforcement and
secondary-decision rollback.

The same database gate covers privacy-field rejection at report intake, exact
anonymous replay, published identity binding, operator scope/role/2FA checks,
SLA queue ordering, Team-isolated notices, one-time appeals, atomic appeal
resolution, revision immutability, and absence of identity/network columns in
the report table.

Identity account linking, invitations, member suspension, Portal sessions, dynamic malware
intelligence, production reviewer SSO, production object storage, general outbox
delivery and garbage collection are not implemented by this HTTP slice.
Isolated signing/Catalog publication and transparency logging are implemented by
the S5G/S5H Publisher boundary described in `STORE-PUBLISHER.md`.
