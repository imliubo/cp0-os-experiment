# Store Submission Withdrawal

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-SUBMISSION-WITHDRAWAL.zh-CN.md)

S5J implements the frozen `POST /v1/submissions/{submission_id}:withdraw`
control-plane operation. Withdrawal closes one immutable revision; it does not
delete uploaded content or rewrite its history.

## HTTP contract

- bearer token: live owner/developer with `store.submit`, or internal
  `store.control`;
- live team membership and enabled 2FA are re-read inside the transaction;
- `Idempotency-Key`: required, 16-128 bytes;
- `If-Match`: required strong ETag for the current resource version;
- request body: exactly empty;
- success: `200 application/json`, updated Submission, and its new strong ETag.

The idempotency request digest binds the operation, Submission ID, and expected
resource version. An exact retry returns the stored body and ETag without
emitting another event. Reusing the key for another request is rejected.
Cross-team lookups return `not-found` instead of disclosing the Submission.

## State and atomic effects

Withdrawal is valid from `draft`, `uploading`, `processing`,
`ready-for-review`, `in-review`, and `pending-secondary-review`. `needs-changes`, `approved`, `rejected`,
and `withdrawn` are terminal and return `invalid-transition`.

One PostgreSQL `SERIALIZABLE` transaction:

1. locks the owner-scoped Submission and verifies its ETag;
2. advances its resource version by exactly one and changes state to
   `withdrawn`;
3. changes any `queued` or `running` scan job to terminal `cancelled`, clears
   its lease, and records `submission-withdrawn`;
4. changes every active review assignment to `cancelled`;
5. marks an unconsumed `submission.scan-requested` outbox event handled so no
   new scan job can be created;
6. completes the idempotency record and appends the audit/outbox mutation;
7. commits all effects together.

The database rejects illegal Submission transitions, resource-version jumps,
scan-job resurrection, and deletion of scan jobs. A Scanner racing withdrawal
must either commit its processing result first or observe the changed state,
version, or lease and fail the stale commit. Review decisions use the same
Submission row lock and ETag rule.

## Retained evidence

Withdrawal never deletes package, Listing, assets, upload chunk descriptors,
scan results, review messages, decisions, assignments, audit events, or outbox
history. A later correction is a new incremented revision and a new review.

## Verification

The ignored PostgreSQL acceptance test covers empty-body enforcement, live
RBAC/2FA/team isolation, stale ETags, exact replay, terminal-state rejection,
queued/running scan cancellation, pending outbox suppression, active review
cancellation, and an injected audit failure proving complete rollback.

```sh
CP0_STORE_TEST_DATABASE_URL=postgres://... \
cargo +1.85.1 test -p cp0-store-control-server --test postgres -- --ignored --nocapture
```
