# Store Review Backend

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-REVIEW-BACKEND.zh-CN.md)

S5E adds the first PostgreSQL and HTTP vertical slice for human Store review.
S5L extends it with mandatory independent secondary review and double approval.
S8J adds the immutable review read model, bounded Submission detail API, and
the production-shaped Review Console data path.
It runs in the Store control plane and is never installed on a CardputerZero
device.

## API slice

- `GET /v1/review/submissions?cursor=&limit=` lists claimable
  `ready-for-review` and `pending-secondary-review` Submissions plus the caller's
  active assignment using a stable, bounded cursor. Each `ReviewQueueItem`
  carries `review_stage`, `assigned_to_caller` and the immutable versioned risk
  assessment bound to its Scanner report plus the authoritative App display
  name, developer name, and category; a primary reviewer never sees their own
  Submission as a secondary-review candidate. The default page size is 25 and
  the hard limit is 50.
- `GET /v1/review/submissions/{submission_id}` returns the authoritative
  Submission and App summary, bound risk assessment, verified scan summary,
  imports, permissions, findings, assignments, decisions, the latest six
  messages, and the latest 32 security-relevant audit projections. Explicit
  truncation flags distinguish a complete history from a bounded tail.
- `POST /v1/review/submissions/{submission_id}:begin` atomically claims a
  Submission and changes it to `in-review`.
- `POST /v1/review/submissions/{submission_id}/decisions` appends one structured
  `needs-changes`, `approved`, or `rejected` decision and completes the active
  assignment. Primary approval moves to `pending-secondary-review`; only an
  approval from a different secondary reviewer reaches final `approved`.
- `POST /v1/submissions/{submission_id}/messages` appends a developer or assigned
  reviewer message without changing the immutable Submission content or ETag.

Begin and decision mutations require both `Idempotency-Key` and a strong
`If-Match` ETag. Messages require `Idempotency-Key`. Exact retries return the
stored response; key reuse with a different request returns
`idempotency-conflict`.

## Reviewer trust boundary

Reviewers are internal identities, not App team members. `reviewers` and
`reviewer_access_tokens` form a separate identity domain with these invariants:

- reviewer roles are `reviewer`, `senior-reviewer`, or `admin`;
- the identity must be active and have 2FA enabled;
- reviewer tokens contain exactly the `store.review` scope, expire within one
  hour, are stored only as SHA-256 digests, and can only transition to revoked;
- a database advisory lock and cross-table trigger prevent one token digest
  from existing in both developer and reviewer domains;
- shared message authentication rejects an ambiguous credential even if the
  database invariant were bypassed.

Developer messages still use live team membership, owner-team checks,
`store.submit`, and 2FA. Reviewer messages require an existing assignment for
the Submission. A reviewer who did not claim a Submission cannot decide it or
join its message thread.

## Transaction and data model

Every write runs in a PostgreSQL `SERIALIZABLE` transaction. Authentication,
role, 2FA, scope, assignment, current state, ETag, idempotency reservation,
resource mutation, audit event, and outbox event commit together. A row lock on
the Submission guarantees that concurrent begin requests produce one active
assignment for each stage.

Review assignments retain immutable reviewer, kind and source-version bindings
and only transition from `active` to `completed` or `cancelled`. Every append-only
decision has a unique foreign key to its active assignment. Database triggers
enforce primary-before-secondary ordering, reviewer independence, legal state
transitions, and two distinct approvals before final `approved`.

Release creation independently joins the primary and secondary assignments and
their approval decisions; directly writing an `approved` Submission cannot
bypass this gate. Non-approval decisions require at least one unique structured
reason code and a bounded actionable note. S5L applies the stronger two-review
baseline to every Submission. S5N adds the deterministic risk tiers and database
anti-forgery checks described in `STORE-RISK-POLICY.md`.

S8J stores `submission_review_metadata` in the Scan Worker's successful result
transaction. It immutably binds the Submission, ready-for-review scan, App
display name, category, default locale, and creation time. Database constraint
triggers reject mismatched or non-ready scans, and update/delete triggers make
the projection append-only. Queue and detail reads inner-join this projection;
older scans that predate it therefore fail closed and must be scanned again
before they can enter the queue.

S5M adds the standalone React/Vite Review Console with queue stage/search
filters, submitted-screen inspection, exact hashes, scan findings, permissions,
imports, messages, audit history, claim controls and structured decisions. Its
strict API client omits browser credentials and binds claims/decisions to ETags
and idempotency keys. S8I supplies the audience-specific workforce BFF;
S8J removes runtime fixtures, obtains short-lived `store.review` tokens in
memory, reads queue/detail state from Store Control, and refreshes authoritative
state after every mutation. Production IdP/JWKS and deployment remain external
gates.

## Verification

```sh
cargo test -p cp0-store-control-server
cargo clippy -p cp0-store-control-server --all-targets -- -D warnings

# Requires a disposable PostgreSQL 17 database.
CP0_STORE_TEST_DATABASE_URL=postgres://... make store-control-db-check
```

The PostgreSQL gate covers bounded pagination, malformed cursors, strict action
paths, required ETags, live 2FA/expiry/revocation, cross-domain credentials,
exact replay, concurrent claims, primary-reviewer exclusion from secondary
review, assignment authorization, structured decision validation, developer
team isolation, append-only records, injected secondary-decision rollback,
double-approved Release enforcement, database token-domain uniqueness,
immutable review-metadata bindings, fail-closed legacy scans, detail bounds,
and scan-report digest/risk revalidation.
