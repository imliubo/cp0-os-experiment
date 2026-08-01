# Store Portal Identity v1

## Boundary

The Developer Portal is a browser application behind a dedicated BFF. The BFF
is the only component that participates in external OpenID Connect and the only
component that receives the browser session cookie. The Store control service
remains a private OAuth resource server. A browser never receives a Store
control token, an OIDC refresh token, an invitation digest, or an external
OIDC subject.

The v1 contract is provider-neutral. Production configuration must supply an
exact issuer allowlist, client registrations, redirect URIs, accepted MFA ACR
values, signing algorithms and bounded clock skew. Authorization Code with
PKCE, state and nonce are mandatory. Implicit and password grants are not
supported.

## Identity model

An Account is a stable Store principal and a Team Membership is a separate,
versioned authorization relationship. An external identity link maps one exact
case-sensitive `(issuer, subject)` pair to one Account. Email is verified
contact and invitation-routing data; it is never a login key and cannot change
an identity mapping.

An Account may have up to eight active external identity links. Adding or
removing a link requires a fresh provider challenge. The last link cannot be
removed. A link already owned by another Account cannot be merged through the
Portal. Account merges and recovery escalation require an offline support
procedure with dual control and are outside v1.

One Account may hold active Memberships in up to eight Teams. Each Membership
retains its existing globally unique `member_id`; an `account_id` binds those
authorization subjects without changing App, audit or access-token ownership.
The BFF selects one active Membership and obtains a short-lived control token
through a private audience-bound exchange. It never forwards that token to the
browser. Suspended and removed Memberships cannot be selected or exchanged.

The browser-facing API never returns `subject`. It exposes only a configured
provider label, a stable opaque link ID, link time and whether the link was used
for the current session.

## Browser session

After a successful callback, the BFF creates a random 256-bit opaque session
secret and stores only its SHA-256 digest. The browser receives it as
`__Host-cp0_portal` with `Secure`, `HttpOnly`, `SameSite=Strict`, `Path=/` and
no `Domain`. The session has a 30-minute idle lifetime and an eight-hour
absolute lifetime. Authentication, privilege change and MFA step-up rotate the
secret; logout and membership suspension/removal revoke it.

Every state-changing JSON request also requires:

- an exact allowed `Origin` and same-origin fetch metadata;
- a random per-session synchronizer value in `X-CSRF-Token`;
- a random `Idempotency-Key` bound to Account, operation and request digest;
- `If-Match` for every versioned Team, invitation or identity-link mutation.

The CSRF value may be returned by `GET /portal/v1/session`; it is not an
authentication credential. It is stored as a digest and rotates with the
session. CORS is disabled. Responses use `Cache-Control: no-store` and a
referrer policy that prevents callback and invitation material from leaking.

The session records the provider `auth_time` only after signature, issuer,
audience, nonce and configured ACR validation. Sensitive operations require
that time to be no more than five minutes old. Session creation time alone is
not MFA proof.

## Invitation lifecycle

Only a current Team Owner with `store.teams.write`, enabled MFA and a fresh
step-up may create or cancel an invitation. Invitation roles are exactly
`developer`, `release-manager` or `viewer`; Owner is deliberately excluded and
must be granted later through the existing protected role-change endpoint.

The BFF generates a 256-bit invitation secret, stores only its digest and sends
the raw value through the configured transactional email worker. Neither the
API response, audit event nor general outbox payload contains the raw secret.
The acceptance URL places the secret in a fragment. The Portal posts it in a
bounded JSON body to `invitations:inspect` or `invitations:accept`; it never
places the secret in a request path, query string, log or referrer.

An invitation expires after seven days and follows one terminal transition:

```text
pending -> accepted
pending -> cancelled
pending -> expired
```

Acceptance requires an active Portal session with a provider-verified email
equal to the normalized invited address. It atomically creates the Membership,
advances the Team version, marks the invitation accepted, and appends audit and
outbox events. A removed Membership is never reactivated; re-invitation awaits
a separately versioned Membership identity design. Exact acceptance replay is
idempotent, while use by another Account, after expiry, or after cancellation
fails closed.

Pending invitations plus active/suspended members are bounded to 100 per Team.
Creation is rate-limited per Team and normalized email before database work.
Production delivery bounce handling and abuse thresholds are deployment
policy, but may only cancel a pending invitation and may not alter Memberships.

Invitation creation, cancellation and acceptance all lock and advance the Team
aggregate by exactly one. Their `If-Match` precondition and response `ETag`
therefore always refer to the Team version. The invitation also has its own
monotonic `resource_version`, returned together with `team_resource_version`,
but that child version is never overloaded into the HTTP `ETag` header.

## Persistence and transactions

The PostgreSQL adapter uses five ownership tables:

| Table | Security invariant |
| --- | --- |
| `portal_accounts` | stable ID; contact changes are versioned, never used as identity |
| `external_identity_links` | immutable issuer/subject ownership; no physical delete |
| `portal_sessions` | secret and CSRF digests only; one-way revocation and bounded lifetimes |
| `oidc_login_transactions` | hashed state/nonce/verifier, exact intent and short expiry |
| `team_invitations` | token digest only; immutable target/role and terminal state machine |

Invitation acceptance, Team/Membership creation, session rotation,
idempotency completion, audit and outbox append occur in one `SERIALIZABLE`
transaction. OIDC token exchange and email delivery are outside database
transactions. Short database leases and deduplication IDs connect those side
effects; retries cannot create another Membership or deliver a different
invitation secret.

The Portal BFF holds no signing or Store publication key and cannot grant
reviewer or operator roles. Reviewer workforce SSO remains a separate issuer,
client, cookie namespace and service boundary.

## API and delivery stages

The browser contract is frozen in
`schemas/store-portal-identity-v1.openapi.json`. Redirect endpoints initiate or
complete OIDC; JSON endpoints cover the current session, link management and
the invitation lifecycle. Error bodies use a closed Problem shape and never
reflect provider tokens, subjects, invitation values or database details.

Implementation proceeds in four acceptance slices:

1. [complete] PostgreSQL identity, OIDC transaction and session state machines
   with SQL bypass tests;
2. [complete] generic OIDC Authorization Code + PKCE callback, cookie/CSRF
   enforcement, rotation, idle/absolute expiry and logout;
3. [complete] invitation create/list/inspect/cancel/accept with email-worker
   handoff, exact replay, Team limits and transaction rollback tests;
4. [in progress] Portal BFF/browser integration, two-provider link/unlink
   recovery tests, session theft containment, and Membership suspension/removal
   propagation are complete; production IdP conformance remains external.

Production login remains disabled until the configured issuer, accepted MFA
assurance, email delivery, account recovery, abuse response and privacy
retention policies pass their external gates.
