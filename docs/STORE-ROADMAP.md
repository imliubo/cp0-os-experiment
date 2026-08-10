# CardputerZero Store Roadmap

<!-- doc-locale: en -->
> **English** | [简体中文](STORE-ROADMAP.zh-CN.md)

This roadmap covers the web frontends, backend, and device. Every phase starts with an
automatically verifiable contract before moving to hardware. Until the current 24-hour
stability run is complete, all work is limited to local source, simulators, protocols, and
build artifacts.

## S0: Secure Installation Foundation (Complete)

- [x] Deterministic `.capp` packages and separate developer/Store signatures.
- [x] Binding among manifest, WASM imports, permissions, and exact review records.
- [x] Ordered, expiring, signed Catalog; public HTTPS restriction; Catalog rollback defense.
- [x] Dedicated `cp0-stored`, resumable download, digest verification, and controlled appd
  handoff.
- [x] appd atomic installation, strict upgrade, historical rollback, and stable app UIDs.
- [x] 320x170 Store list, details, install progress, and offline/expired states.

## S1: Freeze Product and Contracts (In Progress)

- [x] Define responsibilities for Developer Portal, Review Console, Store Operations, and
  the device.
- [x] Define control/publication-plane isolation, signing service, and end-to-end trust chain.
- [x] Define the Today/Apps/Search/Updates small-screen information architecture.
- [x] Freeze the `store-listing-v1` schema, categories, age ratings, and localization bounds.
- [x] Freeze Submission/Review/Release state machines and OpenAPI v1.
- [x] Write engineering policy for Store content, privacy, review, and removal. Production
  text still requires product and legal approval.

Completion criterion: architecture review passes, and schema/OpenAPI have strict parser and
mutation tests.

## S2: On-Device Search and Discovery MVP

- [x] Add a bounded, paginated `search` protocol with query, offset, limit, total, and
  next_offset.
- [x] Run stable local search in `cp0-stored` over a verified Catalog.
- [x] Add `cp0ctl store search`, rejecting mismatched responses and invalid pagination.
- [x] Add Search input, recent queries, and empty-result states to System Shell.
- [x] Add pixel regressions for Search, no results, maximum text, pagination, and stale
  Catalogs.
- [x] Add Today/Apps/Search/Updates segments and strict SemVer update calculation. Catalog v1
  Apps shows every app; the signed category field arrives in S6.

Completion criterion: the maximum 64-app Catalog can be searched within the CM0 memory
budget, and search terms never leave the device.

## S3: Developer Listing and Submission CLI

- [x] Add `store-listing-v1.schema.json` and a strict Rust validator.
- [x] Make `cp0ctl store validate` verify the `.capp`, Listing, resources, and developer
  signature together.
- [x] Add OAuth Device Flow, chunked upload, retry, and finalize to
  `cp0ctl store submit`.
- [x] Output a machine-readable Submission ID, digest, and Portal URL.
- [x] Ensure the CLI neither reads/uploads developer private keys nor logs OAuth tokens.
- [x] Add deterministic fixtures and tests for network recovery, expired tokens, and digest
  mismatches.

Completion criterion: a developer can submit and track a version using only the SDK, CLI,
and browser.

## S4: Developer Portal Frontend

- [x] Build account, Team, role, 2FA, and Developer Key pages.
- [x] Build App ID creation, name validation, and permanent-ownership pages.
- [x] Build editors for versions, Listings, localization, icons/screenshots, and privacy
  statements.
- [x] Build upload progress, automated checks, review messages, and version-status timeline.
- [x] Build controls for publication method, scheduling, phased release, pause, and withdrawal.
- [x] Test accessibility, keyboard operation, and error recovery in desktop and mobile
  browsers.

Completion criterion: the Portal never handles a private key, and every write is idempotent
and auditable.

The current S4 deliverable is a standalone React/Vite frontend with a strict OpenAPI client
and an in-memory mock for the complete workflow. S5 connects real identity, object storage,
and write auditing. The Portal is not included in the device image.

## S5: Review and Publication Backend

S5A adds the `cp0-store-control` transactional-domain core: testable server-side RBAC/2FA,
permanent App IDs, immutable revisions, strict state machines, ETags, idempotent replay,
append-only audit/outbox, and monotonic Catalog sequences.

S5B adds the first PostgreSQL/HTTP vertical slice: App registration/read, live
token/RBAC/2FA/scope validation, `SERIALIZABLE` idempotent transactions, ETags, bounded
Problem responses, append-only database constraints, and concurrency/rollback acceptance.

S5C connects Submission creation, contiguous chunks up to 256 KiB, reads, and finalize for
`cp0ctl`. Objects use content addressing. Finalize independently rereads and recalculates all
objects and the content digest. Concurrent revisions, interrupted ETags, incorrect digests,
and transaction rollback have real PostgreSQL acceptance.

S5O adds a mark-and-sweep tool for the local reference object backend with default dry-run,
a 24-hour age gate, shared/exclusive upload-GC interlocking, and fail-closed path validation.
Production replication and retention policy remain infrastructure gates.

S5D connects the isolated Scanner: outbox leases, read-only object reconstruction, active
Team developer keys, WASM host-import/permission checks, Listing/PNG validation, retry
recovery, and atomic scan results all have real PostgreSQL acceptance. Dynamic malicious
samples and production object storage remain gated below.

S5E adds a separate reviewer identity domain and backend vertical slice. Bounded queues,
unique concurrent claims, structured decisions, append-only developer/reviewer messages,
live 2FA/revocation, ETags, idempotency, audit, and outbox pass real PostgreSQL acceptance.
Risk grading, independent secondary review, dual approval, and Review Console were pending
at that slice and are addressed by later work.

S5F adds the Release control-plane vertical slice. Only an approved Submission can create a
Release. Owner/release-manager roles, 2FA, scope, Team isolation, concurrent uniqueness,
scheduling, publication queueing, pause/resume/remove, failure retry, strong ETags, exact
idempotency, append-only operation records, and atomic audit/outbox pass PostgreSQL 17
acceptance. `publish` enters only `publishing`; S5G Publisher completes publication.

S5G adds an isolated file-key Publisher/Catalog Builder. Outbox leases, non-reusable
sequences, source-object recalculation, developer-key revalidation, dual Store signing, a
bounded deterministic 64-app Catalog, newest-Release projection, immutable generations,
atomic Release/audit/outbox writeback, and `current` crash recovery pass PostgreSQL 17
acceptance.

S5H adds an append-only transparency leaf and signed Merkle checkpoint for every successful
Catalog snapshot. Per-prefix validation, database/file tamper rejection, and transaction
rollback pass PostgreSQL 17 acceptance. The current file key is a constrained reference
implementation; production HSM/key ceremony, compact proofs, and external witnesses remain
open.

S5I connects the `cp0ctl` developer OAuth Device Flow. Ten-minute device codes, slow polling,
idempotent approval/denial, live role/scope/2FA, 15-minute minimum-scope tokens, single-use
concurrent exchange, revocation, digest storage, and atomic audit/outbox pass PostgreSQL 17
acceptance. Account registration, Team management, Portal sessions, and reviewer SSO remain
Identity/Teams work.

S5J completes Submission withdrawal. Owner/developer authorization, live 2FA/scope, strong
ETags, exact idempotency, the legal state graph, and atomic cancellation of scan jobs,
undelivered scan events, and active review assignments pass PostgreSQL 17 acceptance.
Historical objects, scan results, review messages, and audit events remain immutable.

S5K freezes the external OIDC plus Portal BFF identity boundary and connects Team read, Owner
role changes, member suspend/resume, and irreversible member removal. Five-minute MFA
step-up, Team ETags, the active-last-owner rule, immediate member-token revocation,
non-revival of suspended tokens, terminal identity retention, monotonic versions,
idempotency, and audit/outbox rollback pass PostgreSQL 17 acceptance. Portal BFF has strict
external OIDC Authorization Code plus PKCE login/callback, summary sessions, CSRF,
idle/absolute timeouts, idempotent MFA step-up, session rotation, and logout. Invitation
create/list/inspect/cancel/accept, Team aggregate ETags, encrypted mail leases with backoff
and terminal cleanup, seven-day expiration, and post-acceptance session rotation also pass
end-to-end PostgreSQL 17 acceptance. Account-link list/begin/remove, dual-provider recovery,
dependent-session revocation, Membership-state propagation, and the Portal browser adapter
are complete. Production mail-provider/IdP consistency and reviewer SSO remain open.

S5L upgrades every Submission to independent dual review. Primary approval enters
pending-secondary-review; the primary reviewer cannot claim secondary review; only approval
by a different secondary reviewer reaches approved. Decisions bind assignments, and a
Release database trigger revalidates both reviewers and approvals. Concurrent claims, exact
replay, fault rollback, and direct-SQL bypass pass PostgreSQL 17 acceptance. Risk grading and
Review Console were pending at this slice and are addressed later.

S5M adds an independent Review Console with bounded primary, secondary, and my-active
assignment queues; search; scan results; submission screenshots; hashes; permissions/imports;
messages; audit; claiming; and structured decisions. Its strict client binds ETags and
idempotency and sends no browser cookies to the Store Control API. S8I connects the workforce
BFF session adapter, and S8J connects queue, details, claim, messages, and decisions to real
Store Control data. Production IdP deployment remains open.

S5N adds a versioned review-risk policy. The isolated Scanner derives standard, elevated,
or high from real SDK permissions. An append-only assessment binds the scan/report SHA-256,
and PostgreSQL triggers recalculate and reject forgery, reordering, modification, and
deletion. Review Queue, OpenAPI, and Console consume the same result. S8I completes the
production-shaped workforce BFF and frontend adapter; production IdP/JWKS and key custody
remain open.

S6A adds backward-compatible Discovery Catalog v2. Production Publisher derives developer,
subtitle, category, keywords, and age/privacy metadata from review-bound Listings and the
authoritative display name of the App owner's Team, with strict v1/v2 separation.
`cp0-stored` searches developer, category, and keywords locally over the signed Catalog.

S6B adds Catalog v3 rich media. Root summaries bind icons and bounded details inventories;
details bind 320x170 screenshots. Publisher writes packages, images, details, Catalog, and
transparency objects into one immutable generation with progressive v1/v2/v3 support. S6C
implements content-addressed icon/details/screenshot caches, exact digest and PNG/details
identity checks, separate budgets, and screenshot LRU in `cp0-stored`. S6D connects icons,
details, screenshots, permission diffs, and release notes to System Shell over strict IPC.

S8A adds the Catalog v4 editorial layer. Today's lead recommendation, one or two collections,
and up to four apps per collection come from approved, published Releases. Editorial
revisions, Publisher snapshots, device `today` IPC, and 320x170 Shell navigation form a full
vertical path. An invalid reference makes Publisher safely fall back to v3 rather than
publish an expired recommendation.

S6E adds compatible signed root indexes, category indexes, and bounded shards. Publisher
switches deterministically when either 64 items or 48 KiB is reached, and atomically records
every object in the database and immutable generation. After complete validation,
`cp0-stored` atomically caches the generation and exposes at most 1024 items through bounded
eight-item `browse` and `search` pages. System Shell Store Apps uses `browse(all)`, fetching
adjacent pages at boundaries and showing the exact range. Apps and Search share one
eight-item page cache, keeping `cp0_ui` below 64 KiB. The client rejects wrong page lengths,
offset/next values, sort order, or categories.

- [x] Implement the App Registry, Submission, independent dual-review Review, and core
  Release services. App registration, Submission upload/finalize/read/withdraw, Release
  control, developer OAuth Device Flow, Team read/role management, and constrained Publisher
  outbox have PostgreSQL/HTTP acceptance.
- [x] Add upload interlocking, age gating, dry-run, and fail-closed object GC to the local
  content-addressed backend. The local file backend does not replace production replication,
  cross-region recovery, or formal retention policy.
- [ ] Complete Identity self-service. Member suspend/resume/removal, Portal BFF, external
  identity links, invitations, and session v1 boundaries/OpenAPI are frozen, with PostgreSQL
  state-machine and bypass acceptance. Portal BFF OIDC login/session/MFA step-up/logout,
  invitation HTTP/mail worker, external identity links, and the real Portal account-security
  BFF adapter vertical slice are complete. Production mail-provider and IdP consistency
  integration remains open.
- [x] Implement the isolated Scan Worker for package format, WASM, permissions, resources,
  and malicious samples. Current deterministic structure/capability scans use no IP network
  and a read-only object root. Dynamic rules, reputation sources, and the operational
  isolation environment still require productionization.
- [x] Implement Review Console, structured questions/replies, secondary review, dual
  approval, and risk grading.
- [ ] Deploy production workforce SSO for Review Console and perform an access-revocation
  drill. S8I completes dual-Origin BFF, strict frontend session adaptation, and local
  PostgreSQL/HTTP revocation acceptance. Real IdP/JWKS, hosted keys, production domains, and
  on-site revocation evidence remain open.
- [x] Implement immutable publication generations, transactional outbox, append-only audit,
  and transparency log. Transparency v1 covers complete Catalog snapshot history;
  production object storage, compact proofs, and witness/gossip remain infrastructure work.
- [ ] Integrate a production HSM and key ceremony. The isolated file-key reference Signer
  already prevents web services from reading the private key. Provider-neutral ceremony
  evidence v1 freezes dual-control separation, absence of private-key fields, HSM/trust-update
  digests, key/sequence switching, and a bounded validator. HSM selection, a real quorum
  ceremony, and independent audit remain open.
- [x] Implement deterministic Catalog Builder, sequence allocation, publication, and
  emergency withdrawal. Pause/resume/remove create higher sequences, expiration safely
  merges control events, and sequences are never reused.
- [ ] Complete disaster recovery, key rotation, privilege-escalation, and insider-threat
  testing. Device overlap/switchover/old-key revocation semantics and the Catalog-key-rotation
  operations runbook are complete. Production HSM dual-person ceremony, signed OS trust-root
  update, offline-device cohorts, CDN promotion, and independent audit remain external gates.

Completion criterion: only an approved Submission can be published; changing any object
requires a new revision and another review.

## S6: Rich-Media Discovery Catalog

- [x] Add developer, subtitle, category, keywords, and age/privacy metadata in Catalog v2.
  S6A production Publisher emits strict v2 and device services validate v1 and v2. The
  bounded System Shell summary response remains, with rich fields in later slices.
- [x] Define format, dimensions, digest, and cache limits for 32x32/48x48 icons and 320x170
  screenshots. S6B freezes PNG/descriptor/details, single-resource, and total CM0 cache
  budgets and Publisher publishes them atomically.
- [x] Add Today collections and featured sets. S8A completes this through Catalog v4, strict
  `today` IPC, and single-foreground System Shell collection navigation. S7 computes Updates
  from the verified Catalog and appd installation snapshot.
- [x] Add a signed category index. The S6E root signature binds exact category counts and
  shard ordinals. The device accepts only after recalculation from verified apps; `browse`
  IPC and `cp0ctl store browse` paginate all/category eight entries at a time.
- [x] Switch to a signed root index and bounded shards above 64 apps. This also handles a
  Catalog reaching 48 KiB first, with at most 16 shards and 1024 entries. A missing or
  modified shard prevents cache switching.
- [x] Atomically cache Catalogs/resources in `cp0-stored` without letting resource failure
  affect local app startup. S6C best-effort prefetches icons after Catalog commit and caches
  details/screenshots on demand. Truncation, replacement, wrong dimensions or identity, and
  unsafe cache inodes create no final object and neither roll back the Catalog nor block a
  verified package install.
- [x] Add icons, one-page screenshot viewing, and permission diffs to System Shell. S6D uses
  strict details/media IPC, sending each image as one read-only `SCM_RIGHTS` descriptor. The
  Shell rebinds app/version/type/index/dimensions/length and decodes with libpng. Five detail
  pages cover icon, scrollable description, one 320:170 screenshot, upgrade permission diff,
  and release notes while UI state remains below 64 KiB.
- [x] Let System Shell Apps/Search access the complete 1024-entry Catalog through bounded
  pages. Apps uses uncategorized `browse`; Search stays purely local. Switching refetches a
  page rather than retaining a second app array.

Completion criterion: the device rejects any resource modified, truncated, or replaced by
the CDN.

## S7: Download, Update, and Recovery Experience

S7A adds a strict `control` protocol and `paused`/`canceled` states. Pause retains the
digest-named fragment; cancel deletes it under the global job lock; resume binds the current
Catalog app version and package digest; control is rejected after appd handoff. Device, CLI,
and 320x170 Shell use the same closed failure reasons for Catalog changes, stale Catalogs,
races, and network/storage/verification/installer faults. See
`STORE-DOWNLOAD-CONTROL-V1.md`.

S7B adds a strict one-to-eight-item `install-batch` protocol. The daemon atomically accepts
and serially runs the whole batch under one Catalog identity snapshot. Pause, cancel, or
failure of one item does not block later items. Updates includes retryable updates only,
excludes active jobs, and rejects submission against a stale Catalog. See
`STORE-UPDATE-QUEUE-V1.md`.

S7C adds global Store background status. Polling occurs every second on Store pages or while
a job is active, and every five seconds elsewhere. Home, Tasks, and an ordinary foreground
app can display bounded `DL n%`, `INSTALL`, or `QUEUE N` status. Completion is generated only
by a transition for the same App ID/version. Initial Catalog load does not replay historical
notifications, and multiple completions aggregate without preempting permission, document,
or confirmation UI. See `STORE-BACKGROUND-STATUS-V1.md`.

S7D makes installation use a mandatory two-step preflight that binds the signed Catalog
sequence, complete app object, and a single-use 60-second authorization. Before download and
when consuming authorization, the daemon checks root-owned device policy, allowlist,
persistent partition, and peak `/run` space. Shell shows a trusted, Cancel-by-default prompt
only for new permissions and displays policy-blocked permissions and required/available
space. Resume reruns policy and space preflight. See `STORE-INSTALL-PREFLIGHT-V1.md`.

S7E closes interruption-recovery gates. Digest-named fragments resume across daemon restart;
invalid HTTP Range is rejected before writing; a digest mismatch synchronously truncates and
never reaches appd. appd idempotently replays a completely revalidated same-version,
same-content request. At startup the daemon cleans only strictly named stale handoff files.
Process, protocol, and fault-injection tests are complete; S9 still collects real power-loss
evidence. See `STORE-INTERRUPTION-RECOVERY-V1.md`.

S7F adds automatic app update, disabled by default. A private atomic preference and six-hour
throttle survive daemon restart, but candidates are checked only when external power, wired
network with a default route, and independent root-owned policy all permit it. A candidate
must be a strict version upgrade with no new permissions, and a batch contains at most eight
apps. appd exposes only the minimum installation snapshot to the Store UID and rechecks
policy, signature, digest, and version at automatic handoff. See `STORE-AUTO-UPDATE-V1.md`.

- [x] Add stable pause, resume, cancel, and failure-reason protocol.
- [x] Add Updates, per-app update, and a bounded Update All queue.
- [x] Add download status, progress after leaving Store, and install-complete notification.
- [x] Add new-permission confirmation, policy restrictions, and storage-space preflight.
- [x] Verify power loss, network loss, invalid HTTP Range, digest mismatch, and appd-handoff
  crash recovery. Local gates cover daemon restart and disconnect after appd submission;
  S9 retains real CM0 power/network-loss acceptance.
- [x] Keep automatic update disabled by default and enable it explicitly only under external
  power, wired network, and separate device policy.

Completion criterion: no interruption creates a partially installed state or bypasses
revalidation.

## S8: Operational Quality and Privacy

S8A completes the Today editorial control-plane vertical slice. Independent operator-token
domain; role/2FA/state/expiry/revocation/scope checks; initial create and ETag update; exact
idempotency; immutable revision; audit/outbox; deterministic Catalog v4 projection; and
fail-closed v3 degradation pass real PostgreSQL acceptance. Device `cp0-stored` and System
Shell consume Today/collections in sync. See `STORE-EDITORIAL-V1.md`.

S8B completes default-disabled weekly Store aggregation. The device retains only install,
launch, and crash counts and creates no device identity. Policy revocation or consent-off
atomically clears data. Failed retries reuse the random batch. The backend accepts only the
previous complete week and an exact published version, and public aggregates require at
least 20 batches. See `STORE-METRICS-V1.md`.

S8C implements a non-production content-governance vertical slice. Anonymous structured
reports accept no free text, contact information, device identity, IP address, or User-Agent.
The control plane provides a bounded SLA queue, structured developer notification, and one
appeal. Current SLA constants and reason vocabulary are for engineering acceptance only;
production enablement, automatic action, and final policy require product, legal, and
security approval. See `STORE-MODERATION-V1.md`.

S8D freezes search privacy. Queries run only through the local Unix socket over a verified
Catalog, and recent queries live only in the Shell process. Device status, logs, and strict
aggregate metrics cannot contain queries. Metrics consent does not authorize experiments;
any future search experiment needs separate default-off consent, a field allowlist, and an
explicit retention period. See `STORE-SEARCH-PRIVACY-V1.md`.

S8E completes an engineering resilience drill covering 1024 entries/16 shards, CDN
Range/offline/replacement, Publisher transaction rollback and lease replay, independent
PostgreSQL 17 dump/restore, append-only recovery validation, and file-key signer faults. All
failures have repeatable evidence. Production multi-region drills and HSM ceremony remain
gated by infrastructure and the S5 HSM gate. See `STORE-RESILIENCE-DRILL-V1.md`.

S8F adds an independent Store Operations React/Vite MVP with bounded Today editing, a
320x170 device preview, published-Release candidates, SLA moderation queue, and structured
resolution. Its strict client calls only editorial/moderation v1 APIs and uses a short-lived
operator bearer token, `credentials: omit`, strong ETags, random idempotency keys, and a
64 KiB response limit. S8G completes discovery of real published Releases: the control plane
lists only approved, currently published Releases with matching immutable artifacts and the
latest projection per App, using a strict keyset cursor capped at 50. The frontend validates
the backend's `rel_` IDs and response contract. S8J removes runtime fixtures and connects
Today, Release pagination, moderation pagination, and resolution to workforce BFF/Store
Control; dual-person resolution and formal policy enforcement remain disabled.

S8H freezes workforce identity v1 shared by Review Console and Store Operations but strictly
separated by audience. Separate `__Host-` cookies, OIDC state/nonce/PKCE transactions,
15-minute idle and eight-hour absolute sessions, control tokens bound to a session for at
most five minutes, and synchronous cascading from session/identity-link/principal revocation
to tokens are covered by PostgreSQL state machines and real HTTP acceptance. S8I adds the
independent `cp0-store-workforce-server`: dual Origin/callback configuration, strict OIDC plus
MFA login, pre-provisioned identity links, idempotent short tokens/logout, and secret-free
audit pass a fresh PostgreSQL 17 database acceptance. Review/Operations frontends use cookies
only with their BFF; Control API remains `credentials: omit`, and Bearers are cached only in
memory by audience/scope. Production IdP/JWKS, key custody, domain deployment, and on-site
revocation drills remain external gates. See `STORE-WORKFORCE-IDENTITY-V1.md` and
`STORE-WORKFORCE-SERVER.md`.

S8J closes the real-data paths for both internal consoles. Scan Worker writes immutable
review-display metadata in the result-submission transaction. Review Queue/Detail accepts
only data fully bound to a ready-for-review scan, default locale, and risk digest; the server
revalidates report SHA-256 and risk binding on every read. Review Console uses bounded
Queue/Detail APIs, while Store Operations uses real Today/Release/moderation APIs. Each sends
cookies only to its own workforce BFF, keeps the control token in memory, and rereads server
state after every mutation. Pre-upgrade scans missing the new projections do not enter the
review queue and must be rescanned; this is an intentional fail-closed compatibility policy.

- [x] Restrict Today/collection operations to approved Releases.
- [x] Establish minimal, optional, de-identified install, launch, and crash aggregates.
- [x] Do not upload search terms by default; experiments require separate consent and a
  retention period.
- [ ] Establish content reporting, removal appeal, developer notification, and security
  response SLA. S8C supplies structured reports, SLA queue, notification, and appeal APIs.
  Automatic removal, dual-person action approval, external security on-call, and production
  SLA remain open.
- [x] Exercise capacity, CDN failure, database recovery, queue replay, and signing service.
- [ ] Complete an independent privacy, security, and review-fairness assessment.

## S9: Hardware Gate (After Stability Testing)

- [ ] Deploy the latest Store protocol, `cp0-stored`, appd, and System Shell.
- [ ] Verify that App Metrics defaults off with no pending data and that appd's single
  blocking lifecycle observer behaves as designed.
- [ ] Use Camera2 to verify Today/Apps/Search/Updates, details, and download progress.
- [ ] Collect six-step evidence for refresh, resume, install, upgrade, offline cache, and
  expiration rejection.
- [ ] Measure memory, CPU, input latency, and SD writes with the maximum Catalog.
- [ ] Verify Home, Tasks, and permission dialogs during download, plus reboot and power-loss
  recovery.
- [ ] Retrieve all evidence and recompute it with the independent validator before closing
  the Store product gate.
