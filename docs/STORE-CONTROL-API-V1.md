# Store Control API v1

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-CONTROL-API-V1.zh-CN.md)

`schemas/store-control-v1.openapi.json` is the initial control-plane contract for the
Developer Portal, `cp0ctl store submit`, Review Console, and Release Service. The System
Shell and `cp0-stored` on the device do not call this API; they only read the immutable
publication surface.

## Request Constraints

- `cp0ctl` uses the OAuth Device Authorization Grant. Its access token is valid for at most
  one hour and grants only the `store.submit` scope. The CLI neither stores nor uploads the
  developer's private key.
- The current server vertical slice issues a 15-minute token. A device code is valid for ten
  minutes, polling starts at a five-second interval, and each early poll adds five seconds up
  to a 30-second maximum. Approval requires a current owner or developer with `store.submit`
  and 2FA, and writes the audit and outbox records in the same idempotent transaction. See
  `STORE-OAUTH-DEVICE-FLOW.md`.
- Every POST or PUT below `/v1` requires an `Idempotency-Key` of 16 to 128 bytes.
- Operations that modify existing state also require `If-Match`; the service uses the ETag
  or resource version to reject concurrent overwrites.
- An App ID is permanently assigned to one team. A deleted name does not automatically
  become available for registration by another developer.
- The package, Listing, and two to six resource objects are uploaded by declared SHA-256.
  Each PUT uses `Content-Range` to send a contiguous fragment of at most 256 KiB, and
  `Content-SHA256` is that fragment's digest. Replaying the same part and range is idempotent
  only when the digest is identical; different content cannot overwrite it.
- `finalize` rereads every object, verifies its length and digest, calculates the submission
  content digest, and then freezes the revision.
- A `withdraw` request has an empty body and requires both `Idempotency-Key` and the current
  `If-Match`. Success returns `200`, the updated Submission, and a new ETag.

The content digest is SHA-256 over the following exact byte sequence. First write the ASCII
domain `CardputerZero Store submission content v1\0`. Then write the package SHA and Listing
SHA, each as `u64 big-endian length + UTF-8 bytes`. In Listing order, write each icon and
screenshot path and SHA using the same length prefix, followed by `u64 bytes`, `u16 width`,
and `u16 height`, all big-endian. The server must recalculate the digest independently and
must not trust the finalize request.

Errors use bounded `application/problem+json` responses with a stable `code` for CLI
handling. Internal paths, SQL, object-storage keys, tokens, and scanner output must not
appear in `detail`.

## Team and Authentication Context

A Team read returns only the Team of the access token's current member. Cross-Team IDs
uniformly return `not-found`. Changing a member role requires an Owner, the
`store.teams.write` scope, the current Team ETag, an idempotency key, enabled 2FA, and a
trusted IdP assertion that MFA occurred within the last five minutes. Token creation time
alone does not satisfy step-up authentication. A successful change increments the Team and
member versions, revokes every old token for the target member, and atomically writes audit
and outbox records. The last Owner cannot be downgraded.

The Portal's OIDC/BFF, cookie, CSRF, and account-recovery boundaries are documented in
`STORE-IDENTITY-TEAMS.md`. The Store API does not accept passwords, WebAuthn credentials,
or OIDC refresh tokens.

## Submission State Machine

```text
DRAFT -> UPLOADING -> PROCESSING -> READY_FOR_REVIEW -> IN_REVIEW (primary)
  |          |             |               |              |
  +------> WITHDRAWN <------+---------------+              +-> NEEDS_CHANGES
                           +-> NEEDS_CHANGES                +-> REJECTED
                           +-> REJECTED                     +-> WITHDRAWN
                                                          |
                                                          v
                                              PENDING_SECONDARY_REVIEW
                                                          |
                                                          v
                                               IN_REVIEW (secondary)
                                                  |       |       |
                                                  v       v       v
                                             APPROVED  NEEDS_  REJECTED
                                                       CHANGES
```

`NEEDS_CHANGES`, `APPROVED`, `REJECTED`, and `WITHDRAWN` are terminal states for that
revision. To change the package, Listing, or any resource, a developer must create a new,
incremented revision; an old revision cannot return to `READY_FOR_REVIEW`. Review messages
and decisions are append-only events and cannot modify submission content.

Withdrawal sets the revision to `WITHDRAWN`, cancels active scan jobs and review
assignments, and consumes any undelivered `submission.scan-requested` outbox event in one
`SERIALIZABLE` transaction. Completed scans, messages, decisions, uploaded objects, and
audit records are retained. A revision already in `APPROVED`, `REJECTED`, `NEEDS_CHANGES`,
or `WITHDRAWN` cannot be withdrawn. Row locks and the resource version must resolve
concurrent scan or review submission against withdrawal to one result. See
`STORE-SUBMISSION-WITHDRAWAL.md` for the full contract.

Only a revision that passes automatic scanning can enter `READY_FOR_REVIEW`. Only the Review
Service can move a revision into `IN_REVIEW`, `PENDING_SECONDARY_REVIEW`, `APPROVED`,
`NEEDS_CHANGES`, or `REJECTED`. Primary approval moves only to
`PENDING_SECONDARY_REVIEW`; a different reviewer must claim the secondary assignment and
approve before the revision enters `APPROVED`. Release creation revalidates both approvals
from the assignment and decision tables; request fields cannot bypass this check. The
isolated Scanner assigns standard, elevated, or high risk under a versioned policy and binds
the assessment to an append-only record containing the scan-report SHA-256. PostgreSQL
recalculates the policy and rejects forged assessments. Every risk level still requires
independent dual review.

## Release State Machine

```text
READY -> SCHEDULED -> PUBLISHING -> PUBLISHED -> PAUSED
  |          |             |            |          |
  |          +-> READY     +-> PUBLISH_FAILED      +-> PUBLISHED
  |                             |            |          |
  +-----------------------------+------------+----------+-> REMOVED
```

A Release can reference only an `APPROVED` submission. In `PUBLISHING`, the Release Service
issues a digest authorization; after the isolated Signer signs it, the system generates a
higher-sequence Catalog. Failure enters `PUBLISH_FAILED` and cannot be presented as a
published version. After the cause is corrected, `PUBLISH_FAILED` re-enters `PUBLISHING`
with a new ETag and idempotency key without bypassing signing. Pausing, resuming, and
removing a Release each create a higher-sequence Catalog; they neither overwrite published
objects nor roll the sequence back.

## Retry and Audit

The client applies jittered backoff only to network failures, 429 responses, and retryable
5xx errors. It reauthorizes after 401; after 409 or 412, it rereads the resource and ETag and
lets the user decide. For every state change, the server records the actor, old and new
states, object digest, reason, request ID, and idempotency-key hash, then publishes events
through the transactional outbox. The original access token and complete idempotency key
must not enter the audit log.
